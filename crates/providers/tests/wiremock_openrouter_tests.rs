use futures::StreamExt;
use providers::{
    LlmMessage, LlmMessageRole, LlmProvider, LlmRequest, ProviderCapabilities, ProviderStreamEvent,
};
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
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response("Routed response")))
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

    let body = concat!(
        "data: {\"id\":\"gen-abc\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\",\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"gen-abc\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\",\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"gen-abc\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\",\"role\":\"assistant\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7,\"cost\":0.0000021}}\n\n",
        "data: [DONE]\n\n",
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
    let events = stream.collect::<Vec<_>>().await;
    let tokens: Vec<String> = events
        .iter()
        .filter_map(|item| match item.as_ref().expect("stream item") {
            ProviderStreamEvent::Token(token) => Some(token.clone()),
            ProviderStreamEvent::Metadata(_) => None,
        })
        .collect();

    assert_eq!(
        tokens,
        vec!["hello".to_owned(), " world".to_owned()],
        "unexpected tokens: {:?}",
        tokens
    );

    let meta = events
        .into_iter()
        .find_map(|item| match item.expect("stream item") {
            ProviderStreamEvent::Metadata(meta) => Some(meta),
            ProviderStreamEvent::Token(_) => None,
        })
        .expect("expected metadata event");
    let usage = meta.usage.expect("expected usage");
    assert_eq!(usage.total_tokens, 7);
    assert_eq!(usage.prompt_tokens, 5);
    assert_eq!(usage.completion_tokens, 2);
    assert_eq!(
        meta.cost_usd,
        Some(0.0000021),
        "expected cost_usd to be captured"
    );
}

#[tokio::test]
async fn stream_captures_generation_id_from_chunk() {
    let server = MockServer::start().await;

    let body = concat!(
        "data: {\"id\":\"gen-abc123\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\",\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"gen-abc123\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\",\"role\":\"assistant\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6,\"cost\":0.0000012}}\n\n",
        "data: [DONE]\n\n",
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
    let events = stream.collect::<Vec<_>>().await;
    let tokens: Vec<String> = events
        .iter()
        .filter_map(|item| match item.as_ref().expect("stream item") {
            ProviderStreamEvent::Token(token) => Some(token.clone()),
            ProviderStreamEvent::Metadata(_) => None,
        })
        .collect();

    assert_eq!(
        tokens,
        vec!["hi".to_owned()],
        "unexpected tokens: {:?}",
        tokens
    );

    let meta = events
        .into_iter()
        .find_map(|item| match item.expect("stream item") {
            ProviderStreamEvent::Metadata(meta) => Some(meta),
            ProviderStreamEvent::Token(_) => None,
        })
        .expect("expected metadata event");
    assert_eq!(
        meta.generation_id,
        Some("gen-abc123".to_owned()),
        "expected generation_id to be captured"
    );
}

#[tokio::test]
async fn concurrent_streams_keep_metadata_isolated_per_request() {
    let server = MockServer::start().await;

    let body_a = concat!(
        "data: {\"id\":\"gen-a\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"alpha\",\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"gen-a\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\",\"role\":\"assistant\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3,\"cost\":0.1}}\n\n",
        "data: [DONE]\n\n",
    );
    let body_b = concat!(
        "data: {\"id\":\"gen-b\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"beta\",\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"gen-b\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\",\"role\":\"assistant\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":5,\"total_tokens\":9,\"cost\":0.2}}\n\n",
        "data: [DONE]\n\n",
    );

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body_a),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body_b),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let request_a = minimal_request();
    let request_b = minimal_request();

    let (events_a, events_b) = tokio::join!(
        async {
            provider
                .stream(request_a)
                .await
                .expect("stream a")
                .collect::<Vec<_>>()
                .await
        },
        async {
            provider
                .stream(request_b)
                .await
                .expect("stream b")
                .collect::<Vec<_>>()
                .await
        }
    );

    let meta_a = events_a
        .into_iter()
        .find_map(|item| match item.expect("stream a event") {
            ProviderStreamEvent::Metadata(meta) => Some(meta),
            ProviderStreamEvent::Token(_) => None,
        })
        .expect("metadata a");
    let meta_b = events_b
        .into_iter()
        .find_map(|item| match item.expect("stream b event") {
            ProviderStreamEvent::Metadata(meta) => Some(meta),
            ProviderStreamEvent::Token(_) => None,
        })
        .expect("metadata b");

    assert_eq!(meta_a.generation_id.as_deref(), Some("gen-a"));
    assert_eq!(meta_a.usage.expect("usage a").total_tokens, 3);
    assert_eq!(meta_b.generation_id.as_deref(), Some("gen-b"));
    assert_eq!(meta_b.usage.expect("usage b").total_tokens, 9);
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

#[tokio::test]
async fn readiness_reports_not_configured_without_api_key() {
    let provider = OpenRouterProvider::new(
        "https://openrouter.ai/api/v1",
        None,
        "openai/gpt-4o",
        ProviderCapabilities::default(),
        OpenRouterExtras::default(),
    )
    .expect("provider");

    let readiness = provider.readiness().await.expect("readiness");

    assert!(!readiness.configured);
    assert!(!readiness.reachable);
}

#[tokio::test]
async fn readiness_checks_models_endpoint_successfully() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let readiness = provider.readiness().await.expect("readiness");

    assert!(readiness.configured);
    assert!(readiness.reachable);
    assert!(readiness.message.contains("reachable"));
}

#[tokio::test]
async fn readiness_reports_auth_failure_from_models_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri());
    let readiness = provider.readiness().await.expect("readiness");

    assert!(readiness.configured);
    assert!(!readiness.reachable);
    assert!(readiness.message.contains("401"));
}
