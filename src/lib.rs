use std::time::Duration;

use chrono::{DateTime, Utc};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_LEASE_TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope<T> {
    pub operation_id: String,
    pub revision: u64,
    pub payload: T,
}

impl<T> Envelope<T> {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.operation_id.trim().is_empty() {
            Err("operation_id is required")
        } else {
            Ok(())
        }
    }
}

/// Client for the Opto Sync lease and fencing-token process boundary.
///
/// Callers must carry the returned `fencing_token` on every protected write;
/// owning a lease without enforcing its token does not prevent stale writers.
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
    pub fn new(mut base_url: Url, bearer_token: Option<String>) -> Result<Self, SyncError> {
        if !matches!(base_url.scheme(), "http" | "https") || base_url.cannot_be_a_base() {
            return Err(SyncError::Configuration(
                "Opto Sync URL must be an absolute HTTP(S) base URL".into(),
            ));
        }

        base_url.set_query(None);
        base_url.set_fragment(None);
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }

        let http = reqwest::Client::builder()
            .timeout(DEFAULT_REQUEST_TIMEOUT)
            .build()?;
        Ok(Self {
            base_url,
            bearer_token,
            http,
        })
    }

    pub async fn acquire(
        &self,
        key: &str,
        holder: &str,
        ttl: Duration,
    ) -> Result<Option<Lease>, SyncError> {
        validate_identifier("lease key", key, 512)?;
        validate_identifier("lease holder", holder, 256)?;
        let url = self
            .base_url
            .join(&format!("v1/leases/{}", encode_key(key)))?;
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
        validate_identifier("lease key", &lease.key, 512)?;
        let url = self.base_url.join(&format!(
            "v1/leases/{}/{}",
            encode_key(&lease.key),
            lease.lease_id
        ))?;
        let response = self
            .authorize(self.http.patch(url))
            .header("if-match", lease.fencing_token.to_string())
            .json(&serde_json::json!({ "ttl_ms": ttl_millis(ttl)? }))
            .send()
            .await?;
        checked_json(response).await
    }

    pub async fn release(&self, lease: &Lease) -> Result<(), SyncError> {
        validate_identifier("lease key", &lease.key, 512)?;
        let url = self.base_url.join(&format!(
            "v1/leases/{}/{}",
            encode_key(&lease.key),
            lease.lease_id
        ))?;
        let response = self
            .authorize(self.http.delete(url))
            .header("if-match", lease.fencing_token.to_string())
            .send()
            .await?;
        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        Err(remote_error(&response))
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
        Self {
            client,
            lease: Some(lease),
        }
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
        return Err(remote_error(&response));
    }
    Ok(response.json().await?)
}

fn remote_error(response: &reqwest::Response) -> SyncError {
    // Remote bodies can contain provider or proxy details, so they are never
    // copied into logs or errors at this process boundary.
    SyncError::Remote {
        status: response.status().as_u16(),
    }
}

fn ttl_millis(ttl: Duration) -> Result<u64, SyncError> {
    if ttl.is_zero() || ttl > MAX_LEASE_TTL {
        return Err(SyncError::Configuration(
            "lease TTL must be between 1 ms and 1 hour".into(),
        ));
    }
    ttl.as_millis()
        .try_into()
        .map_err(|_| SyncError::Configuration("lease TTL is too large".into()))
}

fn validate_identifier(label: &str, value: &str, max_len: usize) -> Result<(), SyncError> {
    if value.trim().is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(SyncError::Configuration(format!(
            "{label} must be non-empty, bounded, and contain no control characters"
        )));
    }
    Ok(())
}

fn encode_key(key: &str) -> String {
    utf8_percent_encode(key, NON_ALPHANUMERIC).to_string()
}

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("Opto Sync request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Opto Sync rejected the request with HTTP {status}")]
    Remote { status: u16 },
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_operation_id() {
        assert!(
            Envelope {
                operation_id: "".into(),
                revision: 1,
                payload: (),
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn lease_keys_escape_path_separators_and_percent_signs() {
        assert_eq!(
            encode_key("evgl/reconcile/acme%2F0"),
            "evgl%2Freconcile%2Facme%252F0"
        );
    }

    #[test]
    fn rejects_unsafe_lease_parameters() {
        assert!(ttl_millis(Duration::ZERO).is_err());
        assert!(ttl_millis(MAX_LEASE_TTL + Duration::from_millis(1)).is_err());
        assert!(validate_identifier("lease key", "a\nb", 512).is_err());
    }

    #[test]
    fn normalizes_the_base_url_without_losing_its_path() {
        let client = OptoSyncClient::new(
            Url::parse("https://sync.example.test/control-plane").unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            client.base_url.as_str(),
            "https://sync.example.test/control-plane/"
        );
    }
}
