use crate::{
    is_retryable, LlmProvider, LlmRequest, LlmResponse, ProviderCapabilities, ProviderError,
    ProviderHealth, ProviderReadiness, TokenStream,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client, StatusCode};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tokio::time::timeout;

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub client: Client,
    pub capabilities: ProviderCapabilities,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        capabilities: ProviderCapabilities,
    ) -> Result<Self, ProviderError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(capabilities.request_timeout_seconds))
            .build()
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        Ok(Self {
            name: name.into(),
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            model: model.into(),
            client,
            capabilities,
        })
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn request_body<'a>(&self, request: &'a LlmRequest, stream: bool) -> OpenAiChatRequest<'a> {
        OpenAiChatRequest {
            model: self.model.clone(),
            messages: request
                .messages
                .iter()
                .map(|message| OpenAiMessage {
                    role: message.role.as_openai_role(),
                    content: message.content.as_str(),
                })
                .collect(),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream,
            response_format: (request.json_mode && self.capabilities.supports_json_mode).then_some(
                OpenAiResponseFormat {
                    r#type: "json_object",
                },
            ),
        }
    }

    async fn post_chat(
        &self,
        body: OpenAiChatRequest<'_>,
    ) -> Result<reqwest::Response, ProviderError> {
        let mut builder = self.client.post(self.chat_url()).json(&body);
        if let Some(api_key) = &self.api_key {
            builder = builder.bearer_auth(api_key);
        }

        let response = builder
            .send()
            .await
            .map_err(|error| map_reqwest_error(&error))?;

        if !response.status().is_success() {
            let status = response.status();
            if status == StatusCode::TOO_MANY_REQUESTS {
                return Err(ProviderError::RateLimit);
            }
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Status {
                status: status.as_u16(),
                body,
            });
        }

        Ok(response)
    }

    async fn try_generate(&self, request: &LlmRequest) -> Result<LlmResponse, ProviderError> {
        let body = self.request_body(request, false);
        let raw: Value = self
            .post_chat(body)
            .await?
            .json()
            .await
            .map_err(|error| ProviderError::MalformedResponse(error.to_string()))?;
        let text = raw
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::MalformedResponse("missing choices[0].message.content".into())
            })?
            .to_owned();

        Ok(LlmResponse {
            text,
            raw_json: Some(raw),
        })
    }

    async fn try_stream_response(
        &self,
        request: &LlmRequest,
    ) -> Result<reqwest::Response, ProviderError> {
        let body = self.request_body(request, true);
        self.post_chat(body).await
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn health(&self) -> Result<ProviderHealth, ProviderError> {
        let ok = !self.base_url.is_empty() && !self.name.is_empty() && !self.model.is_empty();
        let message = if ok {
            "provider configured".into()
        } else {
            "provider not configured: name, base_url, or model is empty".into()
        };
        Ok(ProviderHealth {
            name: self.name.clone(),
            ok,
            message: Some(message),
        })
    }

    async fn readiness(&self) -> Result<ProviderReadiness, ProviderError> {
        let configured = !self.base_url.is_empty() && !self.name.is_empty();
        if !configured {
            return Ok(ProviderReadiness {
                configured: false,
                reachable: false,
                message: "Provider not configured".into(),
            });
        }
        match self.client.get(&self.base_url).send().await {
            Ok(_) => Ok(ProviderReadiness {
                configured: true,
                reachable: true,
                message: "Provider reachable".into(),
            }),
            Err(e) if e.is_connect() || e.is_timeout() => Ok(ProviderReadiness {
                configured: true,
                reachable: false,
                message: format!("Provider not reachable: {e}"),
            }),
            Err(_) => Ok(ProviderReadiness {
                configured: true,
                reachable: true,
                message: "Provider reachable (returned error response)".into(),
            }),
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, ProviderError> {
        let max_attempts = u32::from(self.capabilities.max_retries) + 1;
        for attempt in 0..max_attempts {
            match self.try_generate(&request).await {
                Ok(response) => return Ok(response),
                Err(error) if !is_retryable(&error) => return Err(error),
                Err(error) if attempt + 1 == max_attempts => return Err(error),
                Err(_) => {
                    let delay_ms = std::cmp::min(100 * (1u64 << attempt), 2000);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
        // Unreachable: the loop always returns or falls into the Err branch on the last attempt.
        unreachable!("retry loop exhausted without returning")
    }

    async fn stream(&self, request: LlmRequest) -> Result<TokenStream, ProviderError> {
        if !self.capabilities.supports_streaming {
            return Err(ProviderError::StreamingUnsupported);
        }

        let max_attempts = u32::from(self.capabilities.max_retries) + 1;
        let mut last_error = ProviderError::Transport("no attempt made".into());
        for attempt in 0..max_attempts {
            match self.try_stream_response(&request).await {
                Ok(response) => {
                    let stream = response.bytes_stream();
                    let idle_timeout = Duration::from_secs(
                        self.capabilities.stream_idle_timeout_seconds,
                    );
                    return Ok(Box::pin(try_stream! {
                        futures::pin_mut!(stream);
                        loop {
                            let chunk = timeout(idle_timeout, stream.next())
                                .await
                                .map_err(|_| ProviderError::StreamIdleTimeout)?;
                            let Some(chunk) = chunk else {
                                break;
                            };
                            let bytes = chunk.map_err(|error| map_reqwest_error(&error))?;
                            let text = String::from_utf8_lossy(&bytes);
                            for line in text.lines() {
                                let Some(data) = line.strip_prefix("data: ") else {
                                    continue;
                                };
                                if data.trim() == "[DONE]" {
                                    continue;
                                }
                                let value: Value = serde_json::from_str(data)
                                    .map_err(|error| ProviderError::MalformedResponse(error.to_string()))?;
                                if let Some(token) = value
                                    .pointer("/choices/0/delta/content")
                                    .and_then(Value::as_str)
                                {
                                    yield token.to_owned();
                                }
                            }
                        }
                    }));
                }
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
}

fn map_reqwest_error(error: &reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else if error.status() == Some(StatusCode::REQUEST_TIMEOUT) {
        ProviderError::Timeout
    } else {
        ProviderError::Transport(error.to_string())
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest<'a> {
    model: String,
    messages: Vec<OpenAiMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<OpenAiResponseFormat>,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct OpenAiResponseFormat {
    r#type: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LlmMessage, LlmMessageRole};

    #[tokio::test]
    async fn health_returns_ok_true_when_base_url_set() {
        let provider = OpenAiCompatibleProvider::new(
            "local",
            "http://localhost:8081/v1/",
            None,
            "local-model",
            ProviderCapabilities::default(),
        )
        .expect("provider");

        let health = provider.health().await.expect("health");

        assert!(health.ok);
        assert_eq!(health.name, "local");
    }

    #[tokio::test]
    async fn health_returns_ok_false_when_base_url_empty() {
        let provider = OpenAiCompatibleProvider::new(
            "local",
            "",
            None,
            "local-model",
            ProviderCapabilities::default(),
        )
        .expect("provider");

        let health = provider.health().await.expect("health");

        assert!(!health.ok);
    }

    #[tokio::test]
    async fn health_returns_ok_false_when_model_empty() {
        let provider = OpenAiCompatibleProvider::new(
            "local",
            "http://localhost:8081/v1/",
            None,
            "",
            ProviderCapabilities::default(),
        )
        .expect("provider");

        let health = provider.health().await.expect("health");

        assert!(!health.ok);
    }

    #[test]
    fn request_body_uses_json_mode_only_when_supported() {
        let provider = OpenAiCompatibleProvider::new(
            "local",
            "http://localhost:8081/v1/",
            None,
            "local-model",
            ProviderCapabilities {
                supports_json_mode: true,
                ..ProviderCapabilities::default()
            },
        )
        .expect("provider");
        let request = LlmRequest {
            messages: vec![LlmMessage {
                role: LlmMessageRole::System,
                content: "Stay in-world.".into(),
            }],
            temperature: None,
            max_tokens: None,
            json_mode: true,
        };

        let body = provider.request_body(&request, false);

        assert_eq!(
            provider.chat_url(),
            "http://localhost:8081/v1/chat/completions"
        );
        assert!(body.response_format.is_some());
        assert_eq!(body.messages[0].role, "system");
    }

    // ── is_retryable classification ──────────────────────────────────────────

    #[test]
    fn transport_error_is_retryable() {
        assert!(is_retryable(&ProviderError::Transport("connection refused".into())));
    }

    #[test]
    fn timeout_is_retryable() {
        assert!(is_retryable(&ProviderError::Timeout));
    }

    #[test]
    fn rate_limit_is_retryable() {
        assert!(is_retryable(&ProviderError::RateLimit));
    }

    #[test]
    fn http_429_status_is_retryable() {
        assert!(is_retryable(&ProviderError::Status {
            status: 429,
            body: String::new(),
        }));
    }

    #[test]
    fn http_503_is_retryable() {
        assert!(is_retryable(&ProviderError::Status {
            status: 503,
            body: String::new(),
        }));
    }

    #[test]
    fn http_400_is_not_retryable() {
        assert!(!is_retryable(&ProviderError::Status {
            status: 400,
            body: String::new(),
        }));
    }

    #[test]
    fn malformed_response_is_not_retryable() {
        assert!(!is_retryable(&ProviderError::MalformedResponse(
            "invalid json".into()
        )));
    }

    #[test]
    fn streaming_unsupported_is_not_retryable() {
        assert!(!is_retryable(&ProviderError::StreamingUnsupported));
    }

    // ── max_retries=0 means a single attempt ─────────────────────────────────

    #[tokio::test]
    async fn max_retries_zero_makes_single_attempt() {
        // We test with a real provider but a URL that will fail immediately.
        // With max_retries=0, generate() must return on the very first failure
        // with no sleep (test must complete in well under 1 s).
        let provider = OpenAiCompatibleProvider::new(
            "local",
            "http://127.0.0.1:1", // nothing listening on port 1
            None,
            "test-model",
            ProviderCapabilities {
                max_retries: 0,
                request_timeout_seconds: 1,
                ..ProviderCapabilities::default()
            },
        )
        .expect("provider");

        let request = LlmRequest {
            messages: vec![LlmMessage {
                role: LlmMessageRole::User,
                content: "hello".into(),
            }],
            temperature: None,
            max_tokens: None,
            json_mode: false,
        };

        let result = provider.generate(request).await;
        assert!(result.is_err(), "expected error from unreachable host");
    }
}
