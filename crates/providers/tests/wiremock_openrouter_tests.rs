use futures::StreamExt;
use providers::{LlmMessage, LlmMessageRole, LlmProvider, LlmRequest, ProviderCapabilities};
use providers::{OpenRouterExtras, OpenRouterProvider};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn openai_response(content: &str) -> serde_json::Value {
    json!({
        "id": "gen-test-id",
        "choices": [{"message": {"content": content}}],
        "usage": {"prompt_tokens": 3, "completion_tokens": 5, "total_tokens": 8}
    })
}

fn provider_for(base_url: &str) -> OpenRouterProvider {
    OpenRouterProvider::new(
        base_url,
        Some("test-api-key".into()),
        "openai/gpt-4o",
        ProviderCapabilities {
            max_retries: 0,
            request_timeout_seconds: 5,
            supports_streaming: true,
            stream_idle_timeout_seconds: 5,
            supports_model_listing: true,
            ..ProviderCapabilities::default()
        },
        OpenRouterExtras::default(),
    )
    .expect("provider")
}

fn provider_with_extras(base_url: &str, extras: OpenRouterExtras) -> OpenRouterProvider {
    OpenRouterProvider::new(
        base_url,
        Some("test-api-key".into()),
        "openai/gpt-4o",
        ProviderCapabilities {
            max_retries: 0,
            request_timeout_seconds: 5,
            supports_streaming: true,
            stream_idle_timeout_seconds: 5,
            supports_model_listing: true,
            ..ProviderCapabilities::default()
        },
        extras,
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
async fn generate_sends_attribution_headers() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("HTTP-Referer", "https://my.app"))
        .and(header("X-Title", "MyApp"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(openai_response("Hello from OpenRouter")),
        )
        .mount(&server)
        .await;

    let extras = OpenRouterExtras {
        http_referer: Some("https://my.app".into()),
        x_title: Some("MyApp".into()),
        ..OpenRouterExtras::default()
    };
    let provider = provider_with_extras(&server.uri(), extras);

    let result = provider.generate(minimal_request()).await;
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
    assert_eq!(result.unwrap().text, "Hello from OpenRouter");
}

#[tokio::test]
async fn generate_sends_provider_routing_in_body() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_string_contains("\"provider\""))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(openai_response("Routed response")),
        )
        .mount(&server)
        .await;

    let routing = json!({
        "order": ["Anthropic", "OpenAI"],
        "allow_fallbacks": false
    });
    let extras = OpenRouterExtras {
        provider_routing: Some(routing),
        ..OpenRouterExtras::default()
    };
    let provider = provider_with_extras(&server.uri(), extras);

    let result = provider.generate(minimal_request()).await;
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

#[tokio::test]
async fn stream_captures_trailing_usage_chunk() {
    let server = MockServer::start().await;

    let body = format!(
        "{}{}{}{}",
        sse_chunk("hello"),
        sse_chunk(" world"),
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n",
        sse_done()
    );

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
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
        "unexpected tokens: {:?}",
        tokens
    );

    let meta = provider.take_stream_metadata();
    assert!(meta.is_some(), "expected metadata to be captured");
    let meta = meta.unwrap();
    let usage = meta.usage.expect("expected usage");
    assert_eq!(usage.total_tokens, 7);
    assert_eq!(usage.prompt_tokens, 5);
    assert_eq!(usage.completion_tokens, 2);
}

#[tokio::test]
async fn stream_captures_usage_chunk_without_choices_key() {
    let server = MockServer::start().await;

    let body = format!(
        "{}{}",
        "data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7},\"id\":\"gen-abc123\"}\n\n",
        sse_done()
    );

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
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

    assert!(tokens.is_empty(), "expected no content tokens, got: {:?}", tokens);

    let meta = provider.take_stream_metadata();
    assert!(meta.is_some(), "expected metadata to be captured");
    let meta = meta.unwrap();
    let usage = meta.usage.expect("expected usage");
    assert_eq!(usage.total_tokens, 7);
    assert_eq!(
        meta.generation_id,
        Some("gen-abc123".into()),
        "expected generation_id to be captured"
    );
}

#[tokio::test]
async fn list_models_parses_openrouter_format() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {
                    "id": "openai/gpt-4o",
                    "name": "GPT-4o",
                    "context_length": 128000,
                    "pricing": {
                        "prompt": "0.000005",
                        "completion": "0.000015"
                    }
                }
            ]
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let models = provider.list_models().await.expect("list_models");

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "openai/gpt-4o");
    assert_eq!(models[0].name, "GPT-4o");
    assert_eq!(models[0].context_length, Some(128000));

    let pricing = models[0].pricing.as_ref().expect("expected pricing");
    assert!(
        (pricing.prompt_usd_per_token - 0.000005_f64).abs() < 1e-10,
        "unexpected prompt price: {}",
        pricing.prompt_usd_per_token
    );
    assert!(
        (pricing.completion_usd_per_token - 0.000015_f64).abs() < 1e-10,
        "unexpected completion price: {}",
        pricing.completion_usd_per_token
    );
}
