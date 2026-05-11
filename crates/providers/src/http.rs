use crate::{is_retryable, ProviderError};
use std::future::Future;
use std::time::Duration;

/// Execute `op` with retry and exponential backoff.
///
/// Retries up to `max_retries` additional attempts (total = max_retries + 1).
/// Delay: min(100 * 2^attempt, 2000) ms between attempts.
/// Only retries when `is_retryable(&err)` returns true.
pub async fn with_retries<F, Fut, T>(
    max_retries: u8,
    mut op: F,
) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    let max_attempts = u32::from(max_retries) + 1;
    let mut last_error = ProviderError::Transport("no attempt made".into());
    for attempt in 0..max_attempts {
        match op().await {
            Ok(value) => return Ok(value),
            Err(error) if !is_retryable(&error) => return Err(error),
            Err(error) if attempt + 1 == max_attempts => return Err(error),
            Err(error) => {
                last_error = error;
                let delay_ms = std::cmp::min(100 * (1u64 << attempt), 2000);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
    Err(last_error)
}

/// Map a reqwest error to a ProviderError.
pub fn map_reqwest_error(error: &reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else if error.status() == Some(reqwest::StatusCode::REQUEST_TIMEOUT) {
        ProviderError::Timeout
    } else {
        ProviderError::Transport(error.to_string())
    }
}

/// Parse a single SSE data line (the `data: ...` stripped content) into
/// its JSON token content at `/choices/0/delta/content`, if present.
///
/// Returns:
/// - `Ok(Some(token))` if a content token was found
/// - `Ok(None)` if the chunk has no content token (e.g., a usage chunk)
/// - `Err(ProviderError::MalformedResponse)` if the JSON is invalid
pub fn parse_sse_data_line(data: &str) -> Result<Option<String>, ProviderError> {
    if data.trim() == "[DONE]" {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(data)
        .map_err(|e| ProviderError::MalformedResponse(e.to_string()))?;
    Ok(value
        .pointer("/choices/0/delta/content")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned))
}
