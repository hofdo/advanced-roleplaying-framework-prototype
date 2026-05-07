mod common;

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use providers::{LlmProvider, LlmRequest, LlmResponse, ProviderCapabilities, ProviderError, ProviderHealth, ProviderReadiness, TokenStream};
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
#[ignore = "requires docker daemon via testcontainers"]
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
    let ctx = postgres_test_context(mock_provider([bad_delta.to_string()]))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
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
#[ignore = "requires docker daemon via testcontainers"]
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
    let ctx = postgres_test_context(mock_provider([kill_turn.to_string(), invalid_turn.to_string()]))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
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
        let (_, body) = send_empty(
            &ctx.router,
            "GET",
            &format!("/admin/sessions/{}/export/raw", session.id),
        )
        .await;
        let v: Value = json_body(&body);
        serde_json::from_value(v["world_state"].clone()).expect("world state")
    };
    let examiner = world_state.npcs.iter().find(|n| n.npc_id == "examiner").expect("examiner");
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
#[ignore = "requires docker daemon via testcontainers"]
async fn provider_failure_returns_502() {
    // Empty provider queue → NoMockResponse → maps to ProviderError → 502 Bad Gateway.
    let ctx = postgres_test_context(mock_provider(Vec::<String>::new()))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
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
#[ignore = "requires docker daemon via testcontainers"]
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
    let ctx = postgres_test_context(mock_provider([raw.to_string()]))
        .await
        .expect("test context");
    let scenario = sample_scenario();

    send_json(&ctx.router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &ctx.router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Debug Turn Test" }),
    )
    .await;
    let session: persistence::SessionRecord = json_body(&session_body);

    let (status, body) = send_json(
        &ctx.router,
        "POST",
        &format!("/admin/sessions/{}/turn/debug", session.id),
        json!({ "input": "I greet the examiner.", "mode": "dialogue" }),
    )
    .await;
    let response: Value = json_body(&body);

    assert_eq!(status, http::StatusCode::OK);
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
#[ignore = "requires docker daemon via testcontainers"]
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
    let ctx = postgres_test_context(mock_provider([raw_turn.to_string()]))
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
