use std::sync::atomic::{AtomicU64, Ordering};

use hmac::{Hmac, Mac};
use reqwest::StatusCode;
use sha2::{Digest, Sha256};

use crate::models::{SyncEnvelope, WidgetPreferences};

const STATUS_PATH: &str = "/v1/status";
const MAX_RESPONSE_BYTES: usize = 128 * 1024;

type HmacSha256 = Hmac<Sha256>;

fn status_url(preferences: &WidgetPreferences) -> Result<String, String> {
    if preferences.server_url.is_empty() {
        return Err("Sync server URL is not configured.".into());
    }
    if !preferences.server_url.starts_with("http://") {
        return Err("Sync server URL must start with http://.".into());
    }
    Ok(format!("{}{}", preferences.server_url, STATUS_PATH))
}

fn hex(bytes: &[u8]) -> String {
    const CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(CHARS[(byte >> 4) as usize] as char);
        result.push(CHARS[(byte & 0x0f) as usize] as char);
    }
    result
}

fn signature(secret: &str, timestamp: i64, body: &[u8]) -> Result<String, String> {
    let body_hash = hex(&Sha256::digest(body));
    let canonical = format!("PUT\n{STATUS_PATH}\n{timestamp}\n{body_hash}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| "Write secret is invalid.".to_string())?;
    mac.update(canonical.as_bytes());
    Ok(format!("v1={}", hex(&mac.finalize().into_bytes())))
}

fn next_revision(counter: &AtomicU64, floor: u64) -> u64 {
    let wall_clock = chrono::Utc::now().timestamp_millis().max(0) as u64;
    loop {
        let current = counter.load(Ordering::Relaxed);
        let next = wall_clock
            .max(current.saturating_add(1))
            .max(floor.saturating_add(1));
        if counter
            .compare_exchange(current, next, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            return next;
        }
    }
}

async fn limited_bytes(response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("Sync server response is too large.".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "Unable to read sync server response.".to_string())?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("Sync server response is too large.".into());
    }
    Ok(bytes.to_vec())
}

pub async fn download_status(
    client: &reqwest::Client,
    preferences: &WidgetPreferences,
) -> Result<SyncEnvelope, String> {
    let response = client
        .get(status_url(preferences)?)
        .send()
        .await
        .map_err(|_| "Unable to reach sync server.".to_string())?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err("Sync server has no snapshot yet.".into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "Sync server returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    let bytes = limited_bytes(response).await?;
    let envelope: SyncEnvelope = serde_json::from_slice(&bytes)
        .map_err(|_| "Sync server returned an incompatible snapshot.".to_string())?;
    if envelope.schema_version != 1 {
        return Err("Sync server returned an unsupported schema version.".into());
    }
    Ok(envelope)
}

async fn put_once(
    client: &reqwest::Client,
    preferences: &WidgetPreferences,
    envelope: &SyncEnvelope,
) -> Result<Option<SyncEnvelope>, String> {
    if preferences.write_secret.is_empty() {
        return Err("Write secret is not configured.".into());
    }
    let body =
        serde_json::to_vec(envelope).map_err(|_| "Unable to encode sync snapshot.".to_string())?;
    let timestamp = chrono::Utc::now().timestamp();
    let response = client
        .put(status_url(preferences)?)
        .header("Content-Type", "application/json")
        .header("X-CQS-Timestamp", timestamp.to_string())
        .header(
            "X-CQS-Signature",
            signature(&preferences.write_secret, timestamp, &body)?,
        )
        .body(body)
        .send()
        .await
        .map_err(|_| "Unable to reach sync server.".to_string())?;
    if response.status() == StatusCode::CONFLICT {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!(
            "Sync server returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    let bytes = limited_bytes(response).await?;
    if bytes.is_empty() {
        return Ok(Some(envelope.clone()));
    }
    let saved = serde_json::from_slice::<SyncEnvelope>(&bytes)
        .map_err(|_| "Sync server returned an incompatible snapshot.".to_string())?;
    Ok(Some(saved))
}

pub async fn upload_status(
    client: &reqwest::Client,
    preferences: &WidgetPreferences,
    revision: &AtomicU64,
    mut envelope: SyncEnvelope,
) -> Result<SyncEnvelope, String> {
    envelope.revision = next_revision(revision, envelope.revision);
    if let Some(saved) = put_once(client, preferences, &envelope).await? {
        return Ok(saved);
    }

    let remote = download_status(client, preferences).await?;
    envelope.revision = next_revision(revision, remote.revision);
    put_once(client, preferences, &envelope)
        .await?
        .ok_or_else(|| "Sync server rejected the new revision twice.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_the_documented_canonical_request() {
        let result =
            signature("test-secret", 1_700_000_000, br#"{"schemaVersion":1}"#).expect("signature");
        assert_eq!(
            result,
            "v1=85c153b794fc4df07218a3d2bfcfd0e8f8bf1003ebb7ea2e283766309424f04b"
        );
    }

    #[test]
    fn revision_is_strictly_increasing() {
        let counter = AtomicU64::new(42);
        let first = next_revision(&counter, 100);
        let second = next_revision(&counter, 0);
        assert!(first > 100);
        assert!(second > first);
    }

    #[test]
    fn only_plain_http_server_urls_are_accepted() {
        let mut prefs = WidgetPreferences::default();
        prefs.server_url = "https://example.test".into();
        assert!(status_url(&prefs).is_err());
        prefs.server_url = "http://example.test:8787".into();
        assert_eq!(
            status_url(&prefs).expect("url"),
            "http://example.test:8787/v1/status"
        );
    }
}
