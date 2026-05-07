use providers::{LlmMessage, LlmMessageRole, LlmProvider, LlmRequest, ProviderCapabilities, ProviderError};
use providers::OpenAiCompatibleProvider;
use serde_json::json;
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

fn minimal_request() -> LlmRequest {
    LlmRequest {
        messages: vec![LlmMessage {
            role: LlmMessageRole::User,
            content: "test".into(),
        }],
        temperature: None,
        max_tokens: None,
        json_mode: false,
    }
}

fn provider_for(base_url: &str, max_retries: u8) -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new(
        "test",
        base_url,
        None,
        "gpt-4o-mini",
        ProviderCapabilities {
            max_retries,
            request_timeout_seconds: 5,
            ..ProviderCapabilities::default()
        },
    )
    .expect("provider")
}

fn openai_response(content: &str) -> serde_json::Value {
    json!({
        "choices": [{"message": {"content": content}}]
    })
}

#[tokio::test]
async fn http_500_returns_status_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri(), 0);
    let result = provider.generate(minimal_request()).await;

    assert!(matches!(result, Err(ProviderError::Status { status: 500, .. })));
}

#[tokio::test]
async fn http_429_returns_rate_limit_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri(), 0);
    let result = provider.generate(minimal_request()).await;

    assert!(matches!(result, Err(ProviderError::RateLimit)));
}

#[tokio::test]
async fn retry_on_500_then_200_succeeds() {
    let server = MockServer::start().await;

    // First call returns 500, second returns 200 with valid body.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("temporary error"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(openai_response("The guildhall falls silent.")),
        )
        .mount(&server)
        .await;

    // max_retries = 1 means 2 total attempts.
    let provider = provider_for(&server.uri(), 1);
    let result = provider.generate(minimal_request()).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().text, "The guildhall falls silent.");
}

#[tokio::test]
async fn malformed_json_response_returns_malformed_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("not valid json at all {{{{"),
        )
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri(), 0);
    let result = provider.generate(minimal_request()).await;

    assert!(matches!(result, Err(ProviderError::MalformedResponse(_))));
}

#[tokio::test]
async fn missing_choices_field_returns_malformed_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"result": "ok"})),
        )
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri(), 0);
    let result = provider.generate(minimal_request()).await;

    assert!(matches!(result, Err(ProviderError::MalformedResponse(_))));
}
