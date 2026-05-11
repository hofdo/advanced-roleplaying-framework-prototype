use futures::StreamExt;
use providers::{LlmMessage, LlmMessageRole, LlmProvider, LlmRequest, ProviderCapabilities};
use providers::LlamaCppProvider;
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

/// Build a provider whose `base_url` is `{server_uri}/v1` so that
/// `root_url()` strips the `/v1` suffix back to `server_uri`.
fn provider_for(server_uri: &str) -> LlamaCppProvider {
    LlamaCppProvider::new(
        "test",
        format!("{}/v1", server_uri),
        None,
        "qwen-7b",
        ProviderCapabilities {
            max_retries: 0,
            request_timeout_seconds: 5,
            supports_streaming: true,
            stream_idle_timeout_seconds: 5,
            supports_model_listing: true,
            ..ProviderCapabilities::default()
        },
    )
    .expect("provider")
}

// ── SSE helpers ──────────────────────────────────────────────────────────────

fn sse_chunk(token: &str) -> String {
    let body = json!({
        "choices": [{"delta": {"content": token}}]
    });
    format!("data: {}\n\n", body)
}

fn sse_done() -> &'static str {
    "data: [DONE]\n\n"
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn health_returns_ok_when_health_endpoint_returns_ok() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let health = provider.health().await.expect("health");

    assert!(health.ok, "expected ok=true, got: {:?}", health);
    assert_eq!(health.name, "test");
}

#[tokio::test]
async fn readiness_extracts_model_from_props() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/props"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "default_generation_settings": {
                "model": "qwen-7b"
            }
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let readiness = provider.readiness().await.expect("readiness");

    assert!(readiness.reachable, "expected reachable=true, got: {:?}", readiness);
    assert!(
        readiness.message.contains("qwen-7b"),
        "message should contain model name, got: {:?}",
        readiness.message
    );
}

#[tokio::test]
async fn readiness_handles_missing_model_field() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/props"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let readiness = provider.readiness().await.expect("readiness");

    assert!(readiness.reachable, "expected reachable=true, got: {:?}", readiness);
    assert!(
        readiness.message.contains("unavailable"),
        "message should mention 'unavailable', got: {:?}",
        readiness.message
    );
}

#[tokio::test]
async fn list_models_parses_openai_format() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [
                {"id": "qwen-7b", "object": "model"}
            ]
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let models = provider.list_models().await.expect("list_models");

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "qwen-7b");
}

#[tokio::test]
async fn stream_filters_think_control_tokens() {
    let server = MockServer::start().await;

    let body = format!(
        "{}{}{}{}{}",
        sse_chunk("<think>"),
        sse_chunk("hello"),
        sse_chunk("</think>"),
        sse_chunk(" world"),
        sse_done()
    );

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let stream = provider.stream(minimal_request()).await.expect("stream");
    let tokens: Vec<String> = stream
        .map(|item| item.expect("stream item"))
        .collect()
        .await;

    assert_eq!(
        tokens,
        vec!["hello".to_owned(), " world".to_owned()],
        "expected control tokens filtered, got: {:?}",
        tokens
    );
}

#[tokio::test]
async fn stream_filters_pipe_delimited_control_tokens() {
    let server = MockServer::start().await;

    let body = format!(
        "{}{}{}{}",
        sse_chunk("<|im_start|>"),
        sse_chunk("hi"),
        sse_chunk("<|im_end|>"),
        sse_done()
    );

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let stream = provider.stream(minimal_request()).await.expect("stream");
    let tokens: Vec<String> = stream
        .map(|item| item.expect("stream item"))
        .collect()
        .await;

    assert_eq!(
        tokens,
        vec!["hi".to_owned()],
        "expected pipe-delimited control tokens filtered, got: {:?}",
        tokens
    );
}
