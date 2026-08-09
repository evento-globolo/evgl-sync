use anyhow::{Context, Result};
use evgl_sync::{LeaseGuard, OptoSyncClient};
use std::time::Duration;
use url::Url;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let base: Url = required("OPTO_SYNC_URL")?.parse()?;
    let client = OptoSyncClient::new(base, std::env::var("OPTO_SYNC_TOKEN").ok());
    let tenant = std::env::var("EVGL_TENANT").unwrap_or_else(|_| "default".into());
    let shard = std::env::var("EVGL_SHARD").unwrap_or_else(|_| "0".into());
    let holder = std::env::var("EVGL_SYNC_HOLDER")
        .unwrap_or_else(|_| format!("evgl-sync-{}", Uuid::new_v4()));
    let key = format!("evgl/reconcile/{tenant}/{shard}");
    let ttl = Duration::from_secs(30);

    let Some(lease) = client.acquire(&key, &holder, ttl).await? else {
        tracing::info!(%key, "another reconciler owns the shard");
        return Ok(());
    };
    let mut guard = LeaseGuard::new(client, lease);
    tracing::info!(
        %key,
        fencing_token = guard.lease().fencing_token,
        "reconciliation lease acquired"
    );

    // The fencing token must accompany every write to the future sync data plane.
    // This scaffold performs a heartbeat until shutdown; provider reconciliation
    // can be attached without changing the lease safety boundary.
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(Duration::from_secs(10)) => {
                guard.renew(ttl).await?;
                tracing::info!(
                    fencing_token = guard.lease().fencing_token,
                    expires_at = %guard.lease().expires_at,
                    "lease renewed"
                );
            }
        }
    }
    guard.release().await?;
    Ok(())
}

fn required(key: &'static str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing {key}"))
}
