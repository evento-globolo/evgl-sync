use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct OptoSyncClient {
    base_url: Url,
    bearer_token: Option<String>,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub key: String,
    pub holder: String,
    pub lease_id: Uuid,
    pub fencing_token: u64,
    pub expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct AcquireRequest<'a> {
    holder: &'a str,
    ttl_ms: u64,
}

impl OptoSyncClient {
    pub fn new(base_url: Url, bearer_token: Option<String>) -> Self {
        Self { base_url, bearer_token, http: reqwest::Client::new() }
    }

    pub async fn acquire(
        &self,
        key: &str,
        holder: &str,
        ttl: Duration,
    ) -> Result<Option<Lease>, SyncError> {
        let url = self.base_url.join(&format!("v1/leases/{}", encode_key(key)))?;
        let request = self.authorize(self.http.put(url)).json(&AcquireRequest {
            holder,
            ttl_ms: ttl_millis(ttl)?,
        });
        let response = request.send().await?;
        if response.status() == StatusCode::CONFLICT {
            return Ok(None);
        }
        Ok(Some(checked_json(response).await?))
    }

    pub async fn renew(&self, lease: &Lease, ttl: Duration) -> Result<Lease, SyncError> {
        let url = self.base_url.join(&format!(
            "v1/leases/{}/{}",
            encode_key(&lease.key), lease.lease_id
        ))?;
        let response = self.authorize(self.http.patch(url))
            .header("if-match", lease.fencing_token.to_string())
            .json(&serde_json::json!({ "ttl_ms": ttl_millis(ttl)? }))
            .send().await?;
        checked_json(response).await
    }

    pub async fn release(&self, lease: &Lease) -> Result<(), SyncError> {
        let url = self.base_url.join(&format!(
            "v1/leases/{}/{}",
            encode_key(&lease.key), lease.lease_id
        ))?;
        let response = self.authorize(self.http.delete(url))
            .header("if-match", lease.fencing_token.to_string())
            .send().await?;
        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        Err(remote_error(response).await)
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.bearer_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }
}

pub struct LeaseGuard {
    client: OptoSyncClient,
    lease: Option<Lease>,
}

impl LeaseGuard {
    pub fn new(client: OptoSyncClient, lease: Lease) -> Self {
        Self { client, lease: Some(lease) }
    }

    pub fn lease(&self) -> &Lease {
        self.lease.as_ref().expect("lease exists until release")
    }

    pub async fn renew(&mut self, ttl: Duration) -> Result<&Lease, SyncError> {
        let next = self.client.renew(self.lease(), ttl).await?;
        self.lease = Some(next);
        Ok(self.lease())
    }

    pub async fn release(mut self) -> Result<(), SyncError> {
        if let Some(lease) = self.lease.take() {
            self.client.release(&lease).await?;
        }
        Ok(())
    }
}

async fn checked_json<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, SyncError> {
    if !response.status().is_success() {
        return Err(remote_error(response).await);
    }
    Ok(response.json().await?)
}

async fn remote_error(response: reqwest::Response) -> SyncError {
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    SyncError::Remote { status, body }
}

fn ttl_millis(ttl: Duration) -> Result<u64, SyncError> {
    ttl.as_millis()
        .try_into()
        .map_err(|_| SyncError::Configuration("lease TTL is too large".into()))
}

fn encode_key(key: &str) -> String {
    // Lease keys remain readable while path separators are escaped.
    key.replace('%', "%25").replace('/', "%2F")
}

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("Opto Sync request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Opto Sync rejected the request ({status}): {body}")]
    Remote { status: u16, body: String },
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lease_keys_escape_path_separators() {
        assert_eq!(encode_key("evgl/reconcile/acme/0"), "evgl%2Freconcile%2Facme%2F0");
    }
}
