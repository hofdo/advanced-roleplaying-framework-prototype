mod common;
#[path = "common/postgres.rs"]
mod common_postgres;

use axum::{body::Body, http::Request};
use common::{
    joined_request_text, json_body, mock_provider, recording_mock_provider, sample_scenario,
    send_empty, send_empty_with_bearer, send_json, turn_responses,
};
use common_postgres::{
    postgres_test_context, postgres_test_context_with_config, send_json_with_bearer,
};
use http_body_util::BodyExt;
use providers::{
    LlmProvider, LlmRequest, LlmResponse, ProviderCapabilities, ProviderError, ProviderHealth,
    ProviderReadiness, TokenStream,
};
use serde_json::{Value, json};
use sqlx::Row;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::time::{Duration, sleep};
use tower::util::ServiceExt;

const ADMIN_TOKEN: &str = "test-admin-token";

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
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
#[ignore = "requires Docker-backed Postgres integration"]
async fn create_scenario_and_session_persist_initial_world_state() {
    let ctx = postgres_test_context(mock_provider(Vec::<String>::new()))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    let (scenario_status, _) = send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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
    assert!(
        state
            .0
            .facts
            .iter()
            .any(|fact| fact.visibility == domain::FactVisibility::GmOnly)
    );

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn projected_world_state_hides_gm_only_facts() {
    let ctx = postgres_test_context(mock_provider(Vec::<String>::new()))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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
#[ignore = "requires Docker-backed Postgres integration"]
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
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Postgres;
    config.admin.enabled = true;
    config.admin.token = Some(ADMIN_TOKEN.into());
    let ctx =
        postgres_test_context_with_config(mock_provider(turn_responses([raw.to_string()])), config)
            .await
            .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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

    let message_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE session_id = $1")
            .bind(session.id)
            .fetch_one(&ctx.pool)
            .await
            .expect("message count");
    let delta_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM world_state_deltas WHERE session_id = $1")
            .bind(session.id)
            .fetch_one(&ctx.pool)
            .await
            .expect("delta count");
    let world_event_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE session_id = $1 AND event_type = 'world_event'",
    )
    .bind(session.id)
    .fetch_one(&ctx.pool)
    .await
    .expect("world event count");
    let turn_finished_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE session_id = $1 AND event_type = 'turn_finished'",
    )
    .bind(session.id)
    .fetch_one(&ctx.pool)
    .await
    .expect("turn_finished event count");
    let version: i64 = sqlx::query_scalar("SELECT version FROM world_states WHERE session_id = $1")
        .bind(session.id)
        .fetch_one(&ctx.pool)
        .await
        .expect("world state version");

    assert_eq!(message_count, 2);
    assert_eq!(delta_count, 1);
    assert_eq!(world_event_count, 1);
    assert_eq!(turn_finished_count, 1);
    assert_eq!(version, 1);

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
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
    let ctx = postgres_test_context(mock_provider(turn_responses([
        turn_one.to_string(),
        turn_two.to_string(),
    ])))
    .await
    .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Guildhall Trial" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (first_status, first_body) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "First turn", "mode": "action" }),
    )
    .await;
    assert_eq!(
        first_status,
        http::StatusCode::OK,
        "first turn failed with body: {}",
        String::from_utf8_lossy(&first_body)
    );

    let second_path = format!("/sessions/{}/turn", session.id);
    let (second_status, body) = {
        let mut last = None;
        for _ in 0..20 {
            let attempt = send_json(
                &ctx.router,
                "POST",
                &second_path,
                json!({ "input": "Second turn", "mode": "dialogue" }),
            )
            .await;
            if attempt.0 == http::StatusCode::CONFLICT {
                last = Some(attempt);
                sleep(Duration::from_millis(50)).await;
                continue;
            }
            last = Some(attempt);
            break;
        }
        last.expect("at least one second-turn attempt")
    };
    assert_eq!(
        second_status,
        http::StatusCode::OK,
        "second turn failed with body: {}",
        String::from_utf8_lossy(&body)
    );
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
#[ignore = "requires Docker-backed Postgres integration"]
async fn timeline_routes_return_public_and_raw_history() {
    let raw = r#"{
        "player_response": "The examiner dismisses the crowd and studies your answer in silence.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": ["The registrar seals the turn in the session ledger."]
        }
    }"#;
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Postgres;
    config.admin.enabled = true;
    config.admin.token = Some(ADMIN_TOKEN.into());
    let ctx =
        postgres_test_context_with_config(mock_provider(turn_responses([raw.to_string()])), config)
            .await
            .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Timeline Session" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (turn_status, _) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I answer without lowering my gaze.", "mode": "dialogue" }),
    )
    .await;
    assert_eq!(turn_status, http::StatusCode::OK);

    let (public_status, public_body) = send_empty(
        &ctx.router,
        "GET",
        &format!("/sessions/{}/timeline", session.id),
    )
    .await;
    let public_timeline: Value = json_body(&public_body);
    let public_entries = public_timeline.as_array().expect("timeline array");
    let public_kinds = public_entries
        .iter()
        .filter_map(|entry| entry["kind"].as_str())
        .collect::<Vec<_>>();

    assert_eq!(public_status, http::StatusCode::OK);
    assert!(public_kinds.contains(&"user_message"));
    assert!(public_kinds.contains(&"assistant_message"));
    assert!(public_kinds.contains(&"world_event"));

    let (raw_status, raw_body) = send_empty_with_bearer(
        &ctx.router,
        "GET",
        &format!("/admin/sessions/{}/timeline/raw", session.id),
        ADMIN_TOKEN,
    )
    .await;
    let raw_timeline: Value = json_body(&raw_body);

    assert_eq!(raw_status, http::StatusCode::OK);
    assert_eq!(raw_timeline["messages"].as_array().unwrap().len(), 2);
    assert_eq!(raw_timeline["deltas"].as_array().unwrap().len(), 1);
    assert!(!raw_timeline["events"].as_array().unwrap().is_empty());

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
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

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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
#[ignore = "requires Docker-backed Postgres integration"]
async fn concurrent_turn_returns_409() {
    let ctx = postgres_test_context(Arc::new(BlockingProvider::new()))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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
            .body(Body::from(
                json!({ "input": "First turn", "mode": "action" }).to_string(),
            ))
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

    async fn readiness(&self) -> Result<ProviderReadiness, ProviderError> {
        Ok(ProviderReadiness {
            configured: true,
            reachable: true,
            message: "blocking mock".into(),
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
            usage: None,
            cost_usd: None,
            generation_id: None,
        })
    }

    async fn stream(&self, _request: LlmRequest) -> Result<TokenStream, ProviderError> {
        Err(ProviderError::StreamingUnsupported)
    }
}

// ---------------------------------------------------------------------------
// Behavioral validation tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn delta_with_unknown_npc_id_returns_422() {
    let bad_delta = r#"{
        "player_response": "The examiner nods.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [{"type":"attitude_changed","npc_id":"nonexistent-npc","attitude":"hostile","reason":"provoked"}],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": []
        }
    }"#;
    let ctx = postgres_test_context(mock_provider(turn_responses([bad_delta.to_string()])))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Validation Test" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (status, _) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I greet them.", "mode": "dialogue" }),
    )
    .await;

    assert_eq!(status, http::StatusCode::UNPROCESSABLE_ENTITY);
    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn dead_npc_attitude_change_returns_422() {
    use domain::{NpcStatus, WorldState};

    // Use a custom provider response that tries to change attitude of the examiner NPC.
    // We need to put the examiner into Dead status first via a valid turn,
    // then try to change its attitude.
    let kill_turn = r#"{
        "player_response": "The examiner collapses.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [{"type":"status_changed","npc_id":"examiner","status":"dead","reason":"slain by player"}],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": ["Examiner slain."]
        }
    }"#;
    let invalid_turn = r#"{
        "player_response": "The dead examiner somehow reacts.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [{"type":"attitude_changed","npc_id":"examiner","attitude":"friendly","reason":"somehow not dead"}],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": []
        }
    }"#;
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Postgres;
    config.admin.enabled = true;
    config.admin.token = Some(ADMIN_TOKEN.into());
    let ctx = postgres_test_context_with_config(
        mock_provider(turn_responses([
            kill_turn.to_string(),
            invalid_turn.to_string(),
        ])),
        config,
    )
    .await
    .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Dead NPC Test" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    // First turn: kill the examiner.
    let (kill_status, _) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I attack the examiner.", "mode": "action" }),
    )
    .await;
    assert_eq!(kill_status, http::StatusCode::OK, "kill turn must succeed");

    // Verify examiner is now dead in world state.
    let world_state: WorldState = {
        let (_, body) = send_empty_with_bearer(
            &ctx.router,
            "GET",
            &format!("/admin/sessions/{}/export/raw", session.id),
            ADMIN_TOKEN,
        )
        .await;
        let v: Value = json_body(&body);
        serde_json::from_value(v["world_state"].clone()).expect("world state")
    };
    let examiner = world_state
        .npcs
        .iter()
        .find(|n| n.npc_id == "examiner")
        .expect("examiner");
    assert_eq!(examiner.status, NpcStatus::Dead);

    // Second turn: try to change dead NPC's attitude — must be rejected.
    let (status, _) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I talk to the corpse.", "mode": "dialogue" }),
    )
    .await;

    assert_eq!(status, http::StatusCode::UNPROCESSABLE_ENTITY);
    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn provider_failure_returns_502() {
    // Empty provider queue → NoMockResponse → maps to ProviderError → 502 Bad Gateway.
    let ctx = postgres_test_context(mock_provider(Vec::<String>::new()))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Provider Fail Test" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (status, _) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I look around.", "mode": "action" }),
    )
    .await;

    assert_eq!(status, http::StatusCode::BAD_GATEWAY);
    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn debug_turn_returns_applied_delta() {
    let raw = r#"{
        "player_response": "The examiner nods cautiously.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [{"type":"standing_changed","faction_id":"guild","standing_delta":-2,"reason":"Suspicious behaviour."}],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": []
        }
    }"#;
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Postgres;
    config.admin.enabled = true;
    config.admin.token = Some(ADMIN_TOKEN.into());
    let ctx =
        postgres_test_context_with_config(mock_provider(turn_responses([raw.to_string()])), config)
            .await
            .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Debug Turn Test" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (status, body) = send_json_with_bearer(
        &ctx.router,
        "POST",
        &format!("/admin/sessions/{}/turn/debug", session.id),
        ADMIN_TOKEN,
        json!({ "input": "I greet the examiner.", "mode": "dialogue" }),
    )
    .await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "debug turn failed with body: {}",
        String::from_utf8_lossy(&body)
    );
    let response: Value = json_body(&body);

    assert!(
        response.get("applied_delta").is_some(),
        "debug response must contain applied_delta"
    );
    assert!(
        response["applied_delta"]["faction_changes"].is_array(),
        "applied_delta must include faction_changes array"
    );
    assert_eq!(response["world_state_version"], 1);
    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn campaign_memory_persists_in_raw_admin_export() {
    let raw = r#"{
        "player_response": "The examiner's tone softens as she studies your restraint.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "summary_update": {
                "summary": "The examiner now regards the player as unexpectedly courteous.",
                "reason": "Carry forward the shift in tone."
            },
            "memory_changes": [
                {
                    "type": "added",
                    "text": "The examiner judges the player by how they treat staff under pressure.",
                    "visibility": "player_known",
                    "importance": 7,
                    "related_entity_ids": ["examiner"],
                    "reason": "The player spoke respectfully to staff."
                }
            ],
            "event_log_entries": ["The examiner quietly revises her impression of the player."]
        }
    }"#;
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Postgres;
    config.admin.enabled = true;
    config.admin.token = Some(ADMIN_TOKEN.into());
    let ctx =
        postgres_test_context_with_config(mock_provider(turn_responses([raw.to_string()])), config)
            .await
            .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Campaign Memory" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (turn_status, turn_body) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I thank the staff and keep my voice calm.", "mode": "dialogue" }),
    )
    .await;
    assert_eq!(
        turn_status,
        http::StatusCode::OK,
        "turn failed with body: {}",
        String::from_utf8_lossy(&turn_body)
    );

    let (status, body) = send_empty_with_bearer(
        &ctx.router,
        "GET",
        &format!("/admin/sessions/{}/export/raw", session.id),
        ADMIN_TOKEN,
    )
    .await;
    let payload: Value = json_body(&body);

    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(
        payload["world_state"]["memories"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        payload["world_state"]["memories"][0]["text"],
        "The examiner judges the player by how they treat staff under pressure."
    );

    ctx.cleanup().await;
}

// ---------------------------------------------------------------------------
// Phase 1.3 — export projection and raw_provider_output leak tests
// ---------------------------------------------------------------------------

/// Unit test: no Docker needed.
/// Verifies that projecting a WorldState containing a GmOnly fact with
/// ViewerContext::player() produces a FrontendVisibleState that excludes that
/// fact entirely.
#[test]
fn export_projection_strips_gm_only_facts() {
    use domain::{
        Fact, FactSource, FactVisibility, Scenario, ScenarioType, ViewerContext, WorldState,
    };
    use engine::{BasicFrontendStateProjector, FrontendStateProjector};
    use uuid::Uuid;

    let scenario_id = Uuid::new_v4();
    let scenario = Scenario {
        id: scenario_id,
        title: "Test Scenario".into(),
        scenario_type: ScenarioType::Adventure,
        setting: "test".into(),
        tone: "neutral".into(),
        rules: vec![],
        locations: vec![],
        factions: vec![],
        npcs: vec![],
        quests: vec![],
        secrets: vec![],
        clocks: vec![],
    };
    let world_state = WorldState {
        session_id: Uuid::new_v4(),
        scenario_id,
        version: 1,
        current_location_id: None,
        current_scene: None,
        active_speaker_id: None,
        facts: vec![
            Fact {
                id: "player-fact".into(),
                text: "The hero arrived at the city.".into(),
                visibility: FactVisibility::PlayerKnown,
                known_by: vec![],
                source: FactSource::Scenario,
                reveal_conditions: vec![],
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            },
            Fact {
                id: "gm-only-fact".into(),
                text: "The villain controls the guild secretly.".into(),
                visibility: FactVisibility::GmOnly,
                known_by: vec![],
                source: FactSource::Scenario,
                reveal_conditions: vec![],
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            },
        ],
        npcs: vec![],
        factions: vec![],
        quests: vec![],
        clocks: vec![],
        relationships: vec![],
        inventory: vec![],
        memories: vec![],
        summary: None,
        recent_events: vec![],
    };

    let visible =
        BasicFrontendStateProjector.project(&scenario, &world_state, &ViewerContext::player());

    // Only the player-known fact must appear; the GM-only fact must be absent.
    assert_eq!(visible.player_known_facts.len(), 1);
    assert_eq!(visible.player_known_facts[0].id, "player-fact");
    assert!(
        visible
            .player_known_facts
            .iter()
            .all(|f| f.id != "gm-only-fact"),
        "GM-only fact must not appear in projected player state"
    );
}

/// Integration test (requires Docker/testcontainers).
/// Creates a session, runs a turn, then asserts:
/// 1. The turn response JSON has no non-null `raw_provider_output` field.
/// 2. The /export response JSON has no `raw_provider_output` field at all.
/// 3. The /export response uses `visible_state` (not `world_state`).
#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn turn_response_and_export_do_not_leak_raw_provider_output() {
    let raw_turn = r#"{
        "player_response": "The examiner nods cautiously.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": []
        }
    }"#;
    let ctx = postgres_test_context(mock_provider(turn_responses([raw_turn.to_string()])))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Leak Test Session" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    // Run a turn and check the turn response JSON.
    let (turn_status, turn_body) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I greet the examiner.", "mode": "dialogue" }),
    )
    .await;
    assert_eq!(turn_status, http::StatusCode::OK);
    let turn_json: Value = json_body(&turn_body);

    // raw_provider_output must either be absent or null in normal turn responses.
    if let Some(rpo) = turn_json.get("raw_provider_output") {
        assert!(
            rpo.is_null(),
            "raw_provider_output must be null or absent in turn response, got: {rpo}"
        );
    }

    // Fetch the export and verify it uses visible_state, not world_state,
    // and contains no raw_provider_output.
    let (export_status, export_body) = send_empty(
        &ctx.router,
        "GET",
        &format!("/sessions/{}/export", session.id),
    )
    .await;
    assert_eq!(export_status, http::StatusCode::OK);
    let export_json: Value = json_body(&export_body);

    // Export must expose visible_state (projected), not the raw world_state.
    assert!(
        export_json.get("visible_state").is_some(),
        "export response must contain visible_state"
    );
    assert!(
        export_json.get("world_state").is_none(),
        "export response must NOT contain raw world_state"
    );

    // No raw_provider_output anywhere in the export payload.
    let export_str = String::from_utf8(export_body.to_vec()).expect("utf8");
    assert!(
        !export_str.contains("\"raw_provider_output\""),
        "export response must not contain raw_provider_output field"
    );

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn raw_provider_output_remains_null_when_storage_disabled() {
    let raw_turn = r#"{
        "player_response": "The examiner nods cautiously.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": []
        }
    }"#;
    let ctx = postgres_test_context(mock_provider(turn_responses([raw_turn.to_string()])))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Raw Output Disabled" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (status, _) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I greet the examiner.", "mode": "dialogue" }),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);

    let row = sqlx::query(
        "SELECT raw_provider_output FROM messages
         WHERE session_id = $1 AND role = 'Assistant'
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(session.id)
    .fetch_one(&ctx.pool)
    .await
    .expect("assistant message row");

    let stored = row
        .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("raw_provider_output")
        .expect("raw provider output column");
    assert_eq!(stored, None);
    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn raw_provider_output_is_persisted_when_storage_enabled() {
    let raw_turn = r#"{
        "player_response": "The examiner nods cautiously.",
        "world_state_delta": {
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": []
        }
    }"#;
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Postgres;
    config.debug.store_raw_provider_output = true;
    let ctx = postgres_test_context_with_config(
        mock_provider(turn_responses([raw_turn.to_string()])),
        config,
    )
    .await
    .expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Raw Output Enabled" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (status, _) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({ "input": "I greet the examiner.", "mode": "dialogue" }),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);

    let row = sqlx::query(
        "SELECT raw_provider_output FROM messages
         WHERE session_id = $1 AND role = 'Assistant'
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(session.id)
    .fetch_one(&ctx.pool)
    .await
    .expect("assistant message row");

    let stored = row
        .try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>("raw_provider_output")
        .expect("raw provider output column")
        .expect("raw output should be stored")
        .0;
    // After the secrecy-boundary split, raw_provider_output captures only the
    // visible-narration response; the oracle delta-extraction call is not
    // persisted as a player-facing artifact.
    assert_eq!(
        stored,
        serde_json::Value::String("The examiner nods cautiously.".to_owned()),
    );
    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "requires Docker-backed Postgres integration"]
async fn postgres_non_streaming_visible_prompt_does_not_receive_gm_only_fact() {
    let visible = "The examiner watches you without lowering her hand from the alarm bell.";
    let empty_delta = r#"{
        "facts_to_add": [],
        "npc_changes": [],
        "faction_changes": [],
        "quest_changes": [],
        "clock_changes": [],
        "relationship_changes": [],
        "location_change": null,
        "event_log_entries": []
    }"#;
    let (provider, recorded_requests) =
        recording_mock_provider([visible.to_owned(), empty_delta.to_owned()]);
    let ctx = postgres_test_context(provider).await.expect("test context");
    let scenario = sample_scenario();

    send_json(
        &ctx.router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Secrecy Boundary Postgres" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (status, _) = send_json(
        &ctx.router,
        "POST",
        &format!("/sessions/{}/turn", session.id),
        json!({
            "input": "I ask the examiner what the soul-mark really means.",
            "mode": "action",
        }),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);

    let requests = recorded_requests.lock().expect("recorded requests");
    assert_eq!(requests.len(), 2);
    assert!(
        !joined_request_text(&requests[0]).contains("soul-mark was not created"),
        "first (visible) request must not include GM-only fact text"
    );
    assert!(
        joined_request_text(&requests[1]).contains("soul-mark was not created"),
        "second (delta-extraction) request must include GM-only fact text"
    );

    // World state must have been mutated successfully (version bumped).
    let version: i64 = sqlx::query_scalar("SELECT version FROM world_states WHERE session_id = $1")
        .bind(session.id)
        .fetch_one(&ctx.pool)
        .await
        .expect("version");
    assert_eq!(version, 1);

    ctx.cleanup().await;
}
