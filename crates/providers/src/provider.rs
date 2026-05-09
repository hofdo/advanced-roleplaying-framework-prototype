use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use thiserror::Error;

pub type TokenStream = Pin<Box<dyn Stream<Item = Result<String, ProviderError>> + Send>>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn health(&self) -> Result<ProviderHealth, ProviderError>;
    async fn readiness(&self) -> Result<ProviderReadiness, ProviderError>;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, ProviderError>;
    async fn stream(&self, request: LlmRequest) -> Result<TokenStream, ProviderError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderHealth {
    pub name: String,
    pub ok: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderReadiness {
    pub configured: bool,
    pub reachable: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub supports_streaming: bool,
    pub supports_json_mode: bool,
    pub supports_tool_calls: bool,
    pub supports_seed: bool,
    pub max_context_tokens: Option<u32>,
    pub request_timeout_seconds: u64,
    pub stream_idle_timeout_seconds: u64,
    pub max_retries: u8,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            supports_streaming: false,
            supports_json_mode: false,
            supports_tool_calls: false,
            supports_seed: false,
            max_context_tokens: None,
            request_timeout_seconds: 120,
            stream_idle_timeout_seconds: 30,
            max_retries: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmRequest {
    pub messages: Vec<LlmMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub json_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmMessage {
    pub role: LlmMessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmMessageRole {
    System,
    User,
    Assistant,
}

impl LlmMessageRole {
    pub fn as_openai_role(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmResponse {
    pub text: String,
    pub raw_json: Option<serde_json::Value>,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider transport error: {0}")]
    Transport(String),
    #[error("provider timeout")]
    Timeout,
    #[error("provider stream idle timeout")]
    StreamIdleTimeout,
    #[error("provider rate-limited (HTTP 429)")]
    RateLimit,
    #[error("provider returned status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("provider response was malformed: {0}")]
    MalformedResponse(String),
    #[error("provider does not support streaming")]
    StreamingUnsupported,
    #[error("mock provider has no queued response")]
    NoMockResponse,
}

/// Returns `true` for errors that are safe to retry (transport failures, timeouts, rate limits,
/// HTTP 5xx server errors). Returns `false` for errors caused by bad input or bad model output,
/// where retrying would produce the same result.
pub fn is_retryable(error: &ProviderError) -> bool {
    match error {
        ProviderError::Transport(_) => true,
        ProviderError::Timeout => true,
        ProviderError::StreamIdleTimeout => true,
        ProviderError::RateLimit => true,
        ProviderError::Status { status, .. } => *status == 429 || *status >= 500,
        ProviderError::MalformedResponse(_) => false,
        ProviderError::StreamingUnsupported => false,
        ProviderError::NoMockResponse => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_errors_are_retryable() {
        assert!(is_retryable(&ProviderError::Transport("err".into())));
        assert!(is_retryable(&ProviderError::Timeout));
        assert!(is_retryable(&ProviderError::StreamIdleTimeout));
        assert!(is_retryable(&ProviderError::RateLimit));
    }

    #[test]
    fn http_5xx_and_429_are_retryable() {
        assert!(is_retryable(&ProviderError::Status {
            status: 500,
            body: String::new()
        }));
        assert!(is_retryable(&ProviderError::Status {
            status: 503,
            body: String::new()
        }));
        assert!(is_retryable(&ProviderError::Status {
            status: 429,
            body: String::new()
        }));
    }

    #[test]
    fn model_output_and_client_errors_are_not_retryable() {
        assert!(!is_retryable(&ProviderError::MalformedResponse(
            "bad json".into()
        )));
        assert!(!is_retryable(&ProviderError::StreamingUnsupported));
        assert!(!is_retryable(&ProviderError::Status {
            status: 400,
            body: String::new()
        }));
        assert!(!is_retryable(&ProviderError::Status {
            status: 404,
            body: String::new()
        }));
        assert!(!is_retryable(&ProviderError::NoMockResponse));
    }
}
