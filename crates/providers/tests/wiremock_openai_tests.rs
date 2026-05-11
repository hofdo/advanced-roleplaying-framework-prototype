use futures::StreamExt;
use providers::OpenAiCompatibleProvider;
use providers::{
    LlmMessage, LlmMessageRole, LlmProvider, LlmRequest, ProviderCapabilities, ProviderError,
    ProviderStreamEvent,
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::{Duration, sleep},
};
use wiremock::matchers::{method, path};
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

    assert!(matches!(
        result,
        Err(ProviderError::Status { status: 500, .. })
    ));
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
        .respond_with(ResponseTemplate::new(200).set_body_string("not valid json at all {{{{"))
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
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "ok"})))
        .mount(&server)
        .await;

    let provider = provider_for(&server.uri(), 0);
    let result = provider.generate(minimal_request()).await;

    assert!(matches!(result, Err(ProviderError::MalformedResponse(_))));
}

#[tokio::test]
async fn stream_idle_timeout_returns_distinct_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let address = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut request_buffer = vec![0_u8; 4096];
        let _ = socket.read(&mut request_buffer).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
            )
            .await
            .expect("write headers");
        sleep(Duration::from_millis(200)).await;
    });

    let provider = OpenAiCompatibleProvider::new(
        "test",
        format!("http://{address}"),
        None,
        "gpt-4o-mini",
        ProviderCapabilities {
            supports_streaming: true,
            request_timeout_seconds: 5,
            stream_idle_timeout_seconds: 0,
            ..ProviderCapabilities::default()
        },
    )
    .expect("provider");

    let mut stream = provider
        .stream(minimal_request())
        .await
        .expect("stream created");
    let result = stream.next().await.expect("stream item");

    assert!(matches!(result, Err(ProviderError::StreamIdleTimeout)));
}

#[tokio::test]
async fn stream_reassembles_fragmented_sse_data_lines() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let address = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut request_buffer = vec![0_u8; 4096];
        let _ = socket.read(&mut request_buffer).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
            )
            .await
            .expect("write headers");

        let first = "data: {\"choices\":[{\"delta\":{\"cont";
        let second = "ent\":\"hello\"}}]}\n\n";

        socket
            .write_all(format!("{:X}\r\n{first}\r\n", first.len()).as_bytes())
            .await
            .expect("write first chunk");
        socket
            .write_all(format!("{:X}\r\n{second}\r\n", second.len()).as_bytes())
            .await
            .expect("write second chunk");
        socket
            .write_all(b"E\r\ndata: [DONE]\n\n\r\n0\r\n\r\n")
            .await
            .expect("write done");
    });

    let provider = OpenAiCompatibleProvider::new(
        "test",
        format!("http://{address}"),
        None,
        "gpt-4o-mini",
        ProviderCapabilities {
            supports_streaming: true,
            request_timeout_seconds: 5,
            stream_idle_timeout_seconds: 5,
            ..ProviderCapabilities::default()
        },
    )
    .expect("provider");

    let events = provider
        .stream(minimal_request())
        .await
        .expect("stream created")
        .collect::<Vec<_>>()
        .await;

    assert_eq!(events.len(), 1, "unexpected events: {events:?}");
    assert!(matches!(
        events[0].as_ref().expect("stream event"),
        ProviderStreamEvent::Token(token) if token == "hello"
    ));
}
