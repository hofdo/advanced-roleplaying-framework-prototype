use crate::{
    http, LlmProvider, LlmRequest, LlmResponse, ProviderCapabilities, ProviderError, ProviderHealth,
    ProviderModel, ProviderReadiness, StreamMetadata, TokenStream, TokenUsage,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenRouterExtras {
    pub http_referer: Option<String>,
    pub x_title: Option<String>,
    pub provider_routing: Option<serde_json::Value>,
    #[serde(default = "default_true")]
    pub include_usage: bool,
}

pub struct OpenRouterProvider {
    base_url: String,
    api_key: Option<String>,
    model: String,
    client: reqwest::Client,
    capabilities: ProviderCapabilities,
    http_referer: Option<String>,
    x_title: Option<String>,
    provider_routing: Option<serde_json::Value>,
    include_usage: bool,
    last_stream_metadata: Arc<Mutex<Option<StreamMetadata>>>,
}

impl OpenRouterProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        capabilities: ProviderCapabilities,
        extras: OpenRouterExtras,
    ) -> Result<Self, ProviderError> {
        let base_url = {
            let s = base_url.into();
            if s.is_empty() {
                "https://openrouter.ai/api/v1".to_owned()
            } else {
                s.trim_end_matches('/').to_owned()
            }
        };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(capabilities.request_timeout_seconds))
            .build()
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        Ok(Self {
            base_url,
            api_key,
            model: model.into(),
            client,
            capabilities,
            http_referer: extras.http_referer,
            x_title: extras.x_title,
            provider_routing: extras.provider_routing,
            include_usage: extras.include_usage,
            last_stream_metadata: Arc::new(Mutex::new(None)),
        })
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn models_url(&self) -> String {
        format!("{}/models", self.base_url)
    }

    async fn post_chat(
        &self,
        body: &OpenRouterChatRequest<'_>,
    ) -> Result<reqwest::Response, ProviderError> {
        let mut builder = self.client.post(self.chat_url()).json(body);
        if let Some(api_key) = &self.api_key {
            builder = builder.bearer_auth(api_key);
        }
        if let Some(referer) = &self.http_referer {
            builder = builder.header("HTTP-Referer", referer);
        }
        if let Some(title) = &self.x_title {
            builder = builder.header("X-Title", title);
        }

        let response = builder
            .send()
            .await
            .map_err(|e| http::map_reqwest_error(&e))?;

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

    fn request_body<'a>(&self, request: &'a LlmRequest, stream: bool) -> OpenRouterChatRequest<'a> {
        let stream_options = if stream && self.include_usage {
            Some(StreamOptions { include_usage: true })
        } else {
            None
        };
        OpenRouterChatRequest {
            model: self.model.clone(),
            messages: request
                .messages
                .iter()
                .map(|m| OpenAiMessage {
                    role: m.role.as_openai_role(),
                    content: m.content.as_str(),
                })
                .collect(),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            stream,
            response_format: (request.json_mode && self.capabilities.supports_json_mode)
                .then_some(OpenAiResponseFormat { r#type: "json_object" }),
            provider: self.provider_routing.clone(),
            stream_options,
        }
    }

    async fn try_generate(&self, request: &LlmRequest) -> Result<LlmResponse, ProviderError> {
        let body = self.request_body(request, false);
        let raw: Value = self
            .post_chat(&body)
            .await?
            .json()
            .await
            .map_err(|e| ProviderError::MalformedResponse(e.to_string()))?;

        let text = raw
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::MalformedResponse("missing choices[0].message.content".into())
            })?
            .to_owned();

        let usage = raw.pointer("/usage").and_then(parse_usage_from_value);
        let generation_id = raw
            .pointer("/id")
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        Ok(LlmResponse {
            text,
            raw_json: Some(raw),
            usage,
            cost_usd: None,
            generation_id,
        })
    }

    async fn try_stream_response(
        &self,
        request: &LlmRequest,
    ) -> Result<reqwest::Response, ProviderError> {
        let body = self.request_body(request, true);
        self.post_chat(&body).await
    }
}

fn parse_usage_from_value(usage: &Value) -> Option<TokenUsage> {
    Some(TokenUsage {
        prompt_tokens: usage.pointer("/prompt_tokens")?.as_u64()? as u32,
        completion_tokens: usage.pointer("/completion_tokens")?.as_u64()? as u32,
        total_tokens: usage.pointer("/total_tokens")?.as_u64()? as u32,
    })
}

fn parse_usage_from_chunk(value: &Value) -> Option<TokenUsage> {
    let usage = value.pointer("/usage")?;
    parse_usage_from_value(usage)
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn health(&self) -> Result<ProviderHealth, ProviderError> {
        let ok = !self.base_url.is_empty();
        let message = if ok {
            "OpenRouter provider configured".into()
        } else {
            "OpenRouter provider not configured: base_url is empty".into()
        };
        Ok(ProviderHealth {
            name: "openrouter".into(),
            ok,
            message: Some(message),
        })
    }

    async fn readiness(&self) -> Result<ProviderReadiness, ProviderError> {
        let configured = !self.base_url.is_empty();
        Ok(ProviderReadiness {
            configured,
            reachable: true,
            message: "OpenRouter provider configured".into(),
        })
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, ProviderError> {
        http::with_retries(self.capabilities.max_retries, || self.try_generate(&request)).await
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
        let self_meta = Arc::clone(&self.last_stream_metadata);

        Ok(Box::pin(try_stream! {
            futures::pin_mut!(stream);
            loop {
                let chunk = timeout(idle_timeout, stream.next())
                    .await
                    .map_err(|_| ProviderError::StreamIdleTimeout)?;
                let Some(chunk) = chunk else {
                    break;
                };
                let bytes = chunk.map_err(|e| http::map_reqwest_error(&e))?;
                let text = String::from_utf8_lossy(&bytes);
                for line in text.lines() {
                    let Some(data) = line.strip_prefix("data: ") else {
                        continue;
                    };
                    if data.trim() == "[DONE]" {
                        break;
                    }
                    let value: serde_json::Value = serde_json::from_str(data)
                        .map_err(|e| ProviderError::MalformedResponse(e.to_string()))?;

                    // Check if this is a trailing usage chunk (choices absent or empty, usage present)
                    let choices = value.pointer("/choices");
                    let has_content_choice = choices
                        .and_then(|c| c.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|c| c.pointer("/delta/content"))
                        .is_some();

                    let is_usage_chunk = if has_content_choice {
                        false
                    } else {
                        // Either no choices key, or choices array is empty, and usage is present
                        let no_choices = choices.map_or(true, |c| {
                            c.as_array().map_or(true, |arr| arr.is_empty())
                        });
                        no_choices && value.pointer("/usage").is_some()
                    };

                    if is_usage_chunk {
                        let usage = parse_usage_from_chunk(&value);
                        let generation_id = value
                            .pointer("/id")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned);
                        *self_meta.lock().unwrap() = Some(StreamMetadata {
                            usage,
                            cost_usd: None,
                            generation_id,
                            extra: serde_json::Value::Null,
                        });
                    } else if has_content_choice {
                        let token = value
                            .pointer("/choices/0/delta/content")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned)
                            .unwrap_or_default();
                        yield token;
                    }
                }
            }
        }))
    }

    fn take_stream_metadata(&self) -> Option<StreamMetadata> {
        self.last_stream_metadata.lock().unwrap().take()
    }

    async fn list_models(&self) -> Result<Vec<ProviderModel>, ProviderError> {
        let mut builder = self.client.get(self.models_url());
        if let Some(api_key) = &self.api_key {
            builder = builder.bearer_auth(api_key);
        }

        let response = builder
            .send()
            .await
            .map_err(|e| http::map_reqwest_error(&e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Status {
                status: status.as_u16(),
                body,
            });
        }

        let raw: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::MalformedResponse(e.to_string()))?;

        let data = raw
            .pointer("/data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let models = data
            .iter()
            .filter_map(|item| {
                let id = item.pointer("/id")?.as_str()?;
                if id.is_empty() {
                    return None;
                }
                let name = item
                    .pointer("/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let context_length = item
                    .pointer("/context_length")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32);
                let pricing = (|| {
                    let prompt_str = item.pointer("/pricing/prompt")?.as_str()?;
                    let completion_str = item.pointer("/pricing/completion")?.as_str()?;
                    let prompt_usd_per_token: f64 = prompt_str.parse().ok()?;
                    let completion_usd_per_token: f64 = completion_str.parse().ok()?;
                    Some(crate::ModelPricing {
                        prompt_usd_per_token,
                        completion_usd_per_token,
                    })
                })();
                Some(ProviderModel {
                    id: id.to_owned(),
                    name,
                    context_length,
                    pricing,
                })
            })
            .collect();

        Ok(models)
    }
}

#[derive(Serialize)]
struct OpenRouterChatRequest<'a> {
    model: String,
    messages: Vec<OpenAiMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<OpenAiResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct OpenAiResponseFormat {
    r#type: &'static str,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'static str,
    content: &'a str,
}
