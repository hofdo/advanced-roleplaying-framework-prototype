use axum::{Router, body::Body, http::Request};
use domain::{Scenario, fixtures};
use http_body_util::BodyExt;
use providers::{LlmProvider, LlmRequest, MockProvider, RecordingMockProvider};
use serde::de::DeserializeOwned;
use std::sync::{Arc, Mutex};
use tower::util::ServiceExt;

pub fn mock_provider(responses: impl IntoIterator<Item = String>) -> Arc<dyn LlmProvider> {
    Arc::new(MockProvider::new("mock", responses))
}

/// Build a recording mock provider seeded with `responses`.  Returns the
/// erased provider for wiring into the app router plus a shared handle to the
/// recorded request list for boundary assertions.
#[allow(dead_code)]
pub fn recording_mock_provider(
    responses: impl IntoIterator<Item = String>,
) -> (Arc<dyn LlmProvider>, Arc<Mutex<Vec<LlmRequest>>>) {
    let recorder = RecordingMockProvider::new("mock", responses);
    let requests = recorder.requests();
    (Arc::new(recorder), requests)
}

/// Expand combined `{player_response, world_state_delta}` JSON fixtures into
/// the two responses the non-streaming pipeline now consumes (visible text
/// first, delta JSON second).  Mirrors the historical shape the pipeline used
/// before the secrecy-boundary split so tests don't need rewriting twice.
#[allow(dead_code)]
pub fn turn_responses(combined: impl IntoIterator<Item = String>) -> Vec<String> {
    combined
        .into_iter()
        .flat_map(|raw| {
            let value: serde_json::Value =
                serde_json::from_str(&raw).expect("combined turn fixture must be valid JSON");
            let player_response = value["player_response"]
                .as_str()
                .expect("combined turn fixture missing player_response")
                .to_owned();
            let delta = value["world_state_delta"].clone();
            [player_response, delta.to_string()]
        })
        .collect()
}

/// Concatenate every message body in a recorded request into one searchable
/// string.  Used by secrecy-boundary tests to assert that GM-only facts are
/// present or absent without matching against any specific section header.
#[allow(dead_code)]
pub fn joined_request_text(request: &LlmRequest) -> String {
    request
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn send_json(
    router: &Router,
    method: &str,
    path: &str,
    value: serde_json::Value,
) -> (http::StatusCode, bytes::Bytes) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collection")
        .to_bytes();
    (status, body)
}

pub async fn send_empty(
    router: &Router,
    method: &str,
    path: &str,
) -> (http::StatusCode, bytes::Bytes) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collection")
        .to_bytes();
    (status, body)
}

pub async fn send_empty_with_bearer(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
) -> (http::StatusCode, bytes::Bytes) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collection")
        .to_bytes();
    (status, body)
}

pub fn json_body<T: DeserializeOwned>(body: &[u8]) -> T {
    serde_json::from_slice(body).expect("json body")
}

pub fn sample_scenario() -> Scenario {
    let mut scenario = fixtures::scenario()
        .with_title("Chosen Beyond the Goddess")
        .with_setting("A high fantasy isekai world of sword and magic.")
        .with_secret(
            "void-mark",
            "The player's soul-mark was not created by the goddess.",
        )
        .build();
    scenario.tone = "heroic, consequence-driven, high fantasy".into();
    scenario.npcs[0].initial_location_id = None;
    scenario.secrets[0].reveal_conditions = vec!["a divine relic reacts to the mark".into()];
    scenario.clocks[0].current = 1;
    scenario.clocks[0].max = 6;
    scenario
}
