mod common;

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use providers::{LlmProvider, LlmRequest, LlmResponse, ProviderCapabilities, ProviderError, ProviderHealth, TokenStream};
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use common::{
    json_body, mock_provider, postgres_test_context, sample_scenario, send_empty, send_json,
};
use tower::util::ServiceExt;

#[tokio::test]
#[ignore = "requires docker daemon via testcontainers"]
async fn health_reports_postgres_ok() {
    let ctx = postgres_test_context(mock_provider(Vec::<String>::new()))
        .await
        .expect("test context");

    let (status, body) = send_empty(&ctx.router, "GET", "/health").await;
    let payload: Value = json_body(&body);

    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["database"], "postgres:ok");

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires docker daemon via testcontainers"]
async fn create_scenario_and_session_persist_initial_world_state() {
    let ctx = postgres_test_context(mock_provider(Vec::<String>::new()))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    let (scenario_status, _) =
        send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    assert_eq!(scenario_status, http::StatusCode::OK);

    let (session_status, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Guildhall Trial" }),
    )
    .await;
    assert_eq!(session_status, http::StatusCode::OK);
    let session: persistence::SessionRecord = json_body(&session_body);

    let row = sqlx::query("SELECT version, state FROM world_states WHERE session_id = $1")
        .bind(session.id)
        .fetch_one(&ctx.pool)
        .await
        .expect("world state row");
    let version: i64 = row.get("version");
    let state: sqlx::types::Json<domain::WorldState> = row.get("state");

    assert_eq!(version, 0);
    assert_eq!(state.0.session_id, session.id);
    assert!(state.0.facts.iter().any(|fact| fact.visibility == domain::FactVisibility::GmOnly));

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires docker daemon via testcontainers"]
async fn projected_world_state_hides_gm_only_facts() {
    let ctx = postgres_test_context(mock_provider(Vec::<String>::new()))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Guildhall Trial" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (status, body) = send_empty(
        &ctx.router,
        "GET",
        &format!("/sessions/{}/world-state", session.id),
    )
    .await;
    let projected: domain::FrontendVisibleState = json_body(&body);

    assert_eq!(status, http::StatusCode::OK);
    assert!(projected.player_known_facts.is_empty());

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires docker daemon via testcontainers"]
async fn non_streaming_turn_persists_messages_delta_state_and_events() {
    let raw = r#"{
        "player_response": "The guildhall falls silent as examiners shield nearby civilians.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [{"type":"standing_changed","faction_id":"guild","standing_delta":-5,"reason":"The player caused a panic in public."}],
            "quest_changes": [],
            "clock_changes": [{"type":"advanced","clock_id":"fame","delta":1,"reason":"Many witnesses saw the impossible display."}],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": ["The player revealed abnormal mana during guild registration."]
        }
    }"#;
    let ctx = postgres_test_context(mock_provider([raw.to_string()]))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Guildhall Trial" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (status, body) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I flood the guildhall with infinite mana to prove I am powerful.", "mode": "action" }),
    )
    .await;
    let response: Value = json_body(&body);

    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(response["world_state_version"], 1);

    let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE session_id = $1")
        .bind(session.id)
        .fetch_one(&ctx.pool)
        .await
        .expect("message count");
    let delta_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM world_state_deltas WHERE session_id = $1")
        .bind(session.id)
        .fetch_one(&ctx.pool)
        .await
        .expect("delta count");
    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE session_id = $1")
        .bind(session.id)
        .fetch_one(&ctx.pool)
        .await
        .expect("event count");
    let version: i64 = sqlx::query_scalar("SELECT version FROM world_states WHERE session_id = $1")
        .bind(session.id)
        .fetch_one(&ctx.pool)
        .await
        .expect("world state version");

    assert_eq!(message_count, 2);
    assert_eq!(delta_count, 1);
    assert_eq!(event_count, 1);
    assert_eq!(version, 1);

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires docker daemon via testcontainers"]
async fn second_turn_uses_updated_state() {
    let turn_one = r#"{
        "player_response": "The guild recoils from the mana surge.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [{"type":"standing_changed","faction_id":"guild","standing_delta":-5,"reason":"Public panic."}],
            "quest_changes": [],
            "clock_changes": [{"type":"advanced","clock_id":"fame","delta":1,"reason":"Witnesses saw the event."}],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": ["First turn event."]
        }
    }"#;
    let turn_two = r#"{
        "player_response": "The examiner now addresses you with visible caution.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": ["Second turn event."]
        }
    }"#;
    let ctx = postgres_test_context(mock_provider([turn_one.to_string(), turn_two.to_string()]))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Guildhall Trial" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "First turn", "mode": "action" }),
    )
    .await;
    let (_, body) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "Second turn", "mode": "dialogue" }),
    )
    .await;
    let response: Value = json_body(&body);
    let version: i64 = sqlx::query_scalar("SELECT version FROM world_states WHERE session_id = $1")
        .bind(session.id)
        .fetch_one(&ctx.pool)
        .await
        .expect("world state version");

    assert_eq!(response["world_state_version"], 2);
    assert_eq!(version, 2);

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires docker daemon via testcontainers"]
async fn streaming_turn_emits_tokens_then_final_and_persists_after_final() {
    let delta = r#"{
        "facts_to_add": [],
        "npc_changes": [],
        "faction_changes": [{"type":"standing_changed","faction_id":"guild","standing_delta":-5,"reason":"The crowd panicked."}],
        "quest_changes": [],
        "clock_changes": [{"type":"advanced","clock_id":"fame","delta":1,"reason":"Witnesses saw it."}],
        "relationship_changes": [],
        "location_change": null,
        "event_log_entries": ["Streaming turn event."]
    }"#;
    let ctx = postgres_test_context(mock_provider([
        "The guildhall falls silent".to_string(),
        delta.to_string(),
    ]))
    .await
    .expect("test context");
    let scenario = sample_scenario();

    send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Guildhall Trial" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let request = Request::builder()
        .method("POST")
        .uri(format!("/sessions/{}/turn/stream", session.id))
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "input": "I release a visible surge of mana.", "mode": "action" }).to_string(),
        ))
        .expect("request");
    let response = ctx.router.clone().oneshot(request).await.expect("response");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let text = String::from_utf8(body.to_vec()).expect("utf8");
    let version: i64 = sqlx::query_scalar("SELECT version FROM world_states WHERE session_id = $1")
        .bind(session.id)
        .fetch_one(&ctx.pool)
        .await
        .expect("version");

    assert!(text.contains("event: token"));
    assert!(text.contains("event: final"));
    assert_eq!(version, 1);

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires docker daemon via testcontainers"]
async fn concurrent_turn_returns_409() {
    let ctx = postgres_test_context(Arc::new(BlockingProvider::new())).await.expect("test context");
    let scenario = sample_scenario();

    send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Guildhall Trial" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let router = ctx.router.clone();
    let first = tokio::spawn(async move {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/sessions/{}/turn", session.id))
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "input": "First turn", "mode": "action" }).to_string()))
            .expect("request");
        router.oneshot(request).await.expect("response")
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let (status, _) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "Second turn", "mode": "action" }),
    )
    .await;

    assert_eq!(status, http::StatusCode::CONFLICT);
    first.abort();
    ctx.cleanup().await;
}

struct BlockingProvider {
    calls: AtomicUsize,
}

impl BlockingProvider {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for BlockingProvider {
    async fn health(&self) -> Result<ProviderHealth, ProviderError> {
        Ok(ProviderHealth {
            name: "blocking".into(),
            ok: true,
            message: None,
        })
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }

    async fn generate(&self, _request: LlmRequest) -> Result<LlmResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        Ok(LlmResponse {
            text: r#"{"player_response":"blocked","world_state_delta":{"facts_to_add":[],"npc_changes":[],"faction_changes":[],"quest_changes":[],"clock_changes":[],"relationship_changes":[],"location_change":null,"event_log_entries":[]}}"#.into(),
            raw_json: None,
        })
    }

    async fn stream(&self, _request: LlmRequest) -> Result<TokenStream, ProviderError> {
        Err(ProviderError::StreamingUnsupported)
    }
}
