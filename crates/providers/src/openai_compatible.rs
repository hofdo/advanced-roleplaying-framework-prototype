use crate::{
    LlmProvider, LlmRequest, LlmResponse, ProviderCapabilities, ProviderError, ProviderHealth,
    TokenStream,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{Client, StatusCode};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

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
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Status {
                status: status.as_u16(),
                body,
            });
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn health(&self) -> Result<ProviderHealth, ProviderError> {
        Ok(ProviderHealth {
            name: self.name.clone(),
            ok: true,
            message: Some("configured".into()),
        })
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn generate(&self, request: LlmRequest) -> Result<LlmResponse, ProviderError> {
        let body = self.request_body(&request, false);
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

    async fn stream(&self, request: LlmRequest) -> Result<TokenStream, ProviderError> {
        if !self.capabilities.supports_streaming {
            return Err(ProviderError::StreamingUnsupported);
        }

        let response = self.post_chat(self.request_body(&request, true)).await?;
        let stream = response.bytes_stream();

        Ok(Box::pin(try_stream! {
            futures::pin_mut!(stream);
            while let Some(chunk) = stream.next().await {
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
        }))
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
}
