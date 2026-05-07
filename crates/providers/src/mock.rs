use crate::{
    LlmProvider, LlmRequest, LlmResponse, ProviderCapabilities, ProviderError, ProviderHealth,
    TokenStream,
};
use async_stream::stream;
use async_trait::async_trait;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone)]
pub struct MockProvider {
    name: String,
    capabilities: ProviderCapabilities,
    responses: Arc<Mutex<VecDeque<Result<String, ProviderError>>>>,
}

impl MockProvider {
    pub fn new(name: impl Into<String>, responses: impl IntoIterator<Item = String>) -> Self {
        Self {
            name: name.into(),
            capabilities: ProviderCapabilities {
                supports_streaming: true,
                supports_json_mode: true,
                ..ProviderCapabilities::default()
            },
            responses: Arc::new(Mutex::new(
                responses.into_iter().map(Ok).collect::<VecDeque<_>>(),
            )),
        }
    }

    pub fn push_response(&self, response: impl Into<String>) {
        self.responses
            .lock()
            .expect("mock provider mutex")
            .push_back(Ok(response.into()));
    }

    fn pop_response(&self) -> Result<String, ProviderError> {
        self.responses
            .lock()
            .expect("mock provider mutex")
            .pop_front()
            .unwrap_or(Err(ProviderError::NoMockResponse))
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    async fn health(&self) -> Result<ProviderHealth, ProviderError> {
        Ok(ProviderHealth {
            name: self.name.clone(),
            ok: true,
            message: None,
        })
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }

    async fn generate(&self, _request: LlmRequest) -> Result<LlmResponse, ProviderError> {
        Ok(LlmResponse {
            text: self.pop_response()?,
            raw_json: None,
        })
    }

    async fn stream(&self, _request: LlmRequest) -> Result<TokenStream, ProviderError> {
        let text = self.pop_response()?;
        Ok(Box::pin(stream! {
            for token in text.split_whitespace() {
                yield Ok(format!("{token} "));
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LlmMessage, LlmMessageRole};
    use futures::StreamExt;

    fn request() -> LlmRequest {
        LlmRequest {
            messages: vec![LlmMessage {
                role: LlmMessageRole::User,
                content: "hello".into(),
            }],
            temperature: Some(0.7),
            max_tokens: None,
            json_mode: false,
        }
    }

    #[tokio::test]
    async fn mock_provider_returns_queued_generate_response() {
        let provider = MockProvider::new("mock", ["Seraphyne bows her head.".into()]);

        let response = provider.generate(request()).await.expect("mock response");

        assert_eq!(response.text, "Seraphyne bows her head.");
    }

    #[tokio::test]
    async fn mock_provider_streams_visible_tokens() {
        let provider = MockProvider::new("mock", ["Seraphyne watches carefully".into()]);

        let tokens: Vec<_> = provider
            .stream(request())
            .await
            .expect("stream")
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("tokens");

        assert_eq!(tokens.join(""), "Seraphyne watches carefully ");
    }
}
