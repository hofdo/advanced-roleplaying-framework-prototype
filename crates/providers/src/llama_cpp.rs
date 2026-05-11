use crate::{
    LlmProvider, LlmRequest, LlmResponse, ProviderCapabilities, ProviderError, ProviderHealth,
    ProviderModel, ProviderReadiness, ProviderStreamEvent, TokenStream, http,
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
pub struct LlamaCppProvider {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub client: Client,
    pub capabilities: ProviderCapabilities,
}

impl LlamaCppProvider {
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

    /// `{base_url}/chat/completions`
    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    /// Strip `/v1` suffix from `base_url` to get the server root.
    /// E.g. `http://localhost:8081/v1` → `http://localhost:8081`.
    fn root_url(&self) -> String {
        if let Some(root) = self.base_url.strip_suffix("/v1") {
            root.to_owned()
        } else {
            self.base_url.clone()
        }
    }

    /// `{base_url}/models`
    fn models_url(&self) -> String {
        format!("{}/models", self.base_url)
    }

    /// `{root_url}/health`
    fn health_url(&self) -> String {
        format!("{}/health", self.root_url())
    }

    /// `{root_url}/props`
    fn props_url(&self) -> String {
        format!("{}/props", self.root_url())
    }

    fn request_body<'a>(&self, request: &'a LlmRequest, stream: bool) -> LlamaChatRequest<'a> {
        LlamaChatRequest {
            model: self.model.clone(),
            messages: request
                .messages
                .iter()
                .map(|message| LlamaMessage {
                    role: message.role.as_openai_role(),
                    content: message.content.as_str(),
                })
                .collect(),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream,
            response_format: (request.json_mode && self.capabilities.supports_json_mode).then_some(
                LlamaResponseFormat {
                    r#type: "json_object",
                },
            ),
        }
    }

    async fn post_chat(
        &self,
        body: LlamaChatRequest<'_>,
    ) -> Result<reqwest::Response, ProviderError> {
        let mut builder = self.client.post(self.chat_url()).json(&body);
        if let Some(api_key) = &self.api_key {
            builder = builder.bearer_auth(api_key);
        }

        let response = builder
            .send()
            .await
            .map_err(|error| http::map_reqwest_error(&error))?;

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
            usage: None,
            cost_usd: None,
            generation_id: None,
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

/// Returns true if the token is a llama.cpp control token that should be filtered
/// from the stream: `<think>`, `</think>`, or `<|...|>` pipe-delimited tokens.
fn is_control_token(token: &str) -> bool {
    if token == "<think>" || token == "</think>" {
        return true;
    }
    // Match <|...|> pattern
    if token.starts_with("<|") && token.ends_with("|>") {
        return true;
    }
    false
}

#[async_trait]
impl LlmProvider for LlamaCppProvider {
    async fn health(&self) -> Result<ProviderHealth, ProviderError> {
        let url = self.health_url();
        match self.client.get(&url).send().await {
            Ok(response) if response.status().is_success() => Ok(ProviderHealth {
                name: self.name.clone(),
                ok: true,
                message: Some("provider reachable".into()),
            }),
            Ok(response) => {
                let status = response.status().as_u16();
                Ok(ProviderHealth {
                    name: self.name.clone(),
                    ok: false,
                    message: Some(format!("health endpoint returned status {status}")),
                })
            }
            Err(e) => Ok(ProviderHealth {
                name: self.name.clone(),
                ok: false,
                message: Some(e.to_string()),
            }),
        }
    }

    async fn readiness(&self) -> Result<ProviderReadiness, ProviderError> {
        if self.base_url.is_empty() {
            return Ok(ProviderReadiness {
                configured: false,
                reachable: false,
                message: "not configured".into(),
            });
        }

        let url = self.props_url();
        match self.client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                let body: Value = response.json().await.unwrap_or(Value::Null);
                let model_name = body
                    .pointer("/default_generation_settings/model")
                    .and_then(Value::as_str)
                    .map(str::to_owned);

                let message = match model_name {
                    Some(m) => format!("model loaded: {m}"),
                    None => "reachable (model info unavailable)".into(),
                };

                Ok(ProviderReadiness {
                    configured: true,
                    reachable: true,
                    message,
                })
            }
            Ok(response) => {
                let status = response.status().as_u16();
                Ok(ProviderReadiness {
                    configured: true,
                    reachable: false,
                    message: format!("not reachable: props endpoint returned status {status}"),
                })
            }
            Err(e) => Ok(ProviderReadiness {
                configured: true,
                reachable: false,
                message: format!("not reachable: {e}"),
            }),
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, ProviderError> {
        http::with_retries(self.capabilities.max_retries, || {
            self.try_generate(&request)
        })
        .await
    }

    async fn stream(&self, request: LlmRequest) -> Result<TokenStream, ProviderError> {
        if !self.capabilities.supports_streaming {
            return Err(ProviderError::StreamingUnsupported);
        }

        let response = http::with_retries(self.capabilities.max_retries, || {
            self.try_stream_response(&request)
        })
        .await?;

        let stream = response.bytes_stream();
        let idle_timeout = Duration::from_secs(self.capabilities.stream_idle_timeout_seconds);
        let mut decoder = http::SseLineDecoder::default();
        Ok(Box::pin(try_stream! {
            futures::pin_mut!(stream);
            loop {
                let chunk = timeout(idle_timeout, stream.next())
                    .await
                    .map_err(|_| ProviderError::StreamIdleTimeout)?;
                let Some(chunk) = chunk else {
                    break;
                };
                let bytes = chunk.map_err(|error| http::map_reqwest_error(&error))?;
                let text = String::from_utf8_lossy(&bytes);
                for data in decoder.push(&text) {
                    if let Some(token) = http::parse_sse_data_line(&data)? {
                        if !is_control_token(&token) {
                            yield ProviderStreamEvent::Token(token);
                        }
                    }
                }
            }
        }))
    }

    async fn list_models(&self) -> Result<Vec<ProviderModel>, ProviderError> {
        let url = self.models_url();
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|error| http::map_reqwest_error(&error))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Status { status, body });
        }

        let raw: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::MalformedResponse(e.to_string()))?;

        let data = raw
            .pointer("/data")
            .and_then(Value::as_array)
            .ok_or_else(|| ProviderError::MalformedResponse("missing 'data' array".into()))?;

        let models = data
            .iter()
            .filter_map(|item| {
                item.get("id")
                    .and_then(Value::as_str)
                    .map(|id| ProviderModel {
                        id: id.to_owned(),
                        name: item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        context_length: None,
                        pricing: None,
                    })
            })
            .collect();

        Ok(models)
    }
}

#[derive(Debug, Serialize)]
struct LlamaChatRequest<'a> {
    model: String,
    messages: Vec<LlamaMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<LlamaResponseFormat>,
}

#[derive(Debug, Serialize)]
struct LlamaMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct LlamaResponseFormat {
    r#type: &'static str,
}
