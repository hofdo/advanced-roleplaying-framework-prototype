mod common;

use common::{json_body, memory_test_context, mock_provider, sample_scenario, send_empty, send_json};
use http::StatusCode;
use serde_json::{json, Value};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_returns_memory_database_status() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));

    let (status, body) = send_empty(&router, "GET", "/health").await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["database"], "memory");
}

// ---------------------------------------------------------------------------
// Scenario CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_scenario_returns_created_scenario() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();
    let id = scenario.id;

    let (status, body) =
        send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["id"], id.to_string());
}

#[tokio::test]
async fn list_scenarios_returns_all_created() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));

    let s1 = sample_scenario();
    let mut s2 = sample_scenario();
    s2.id = Uuid::new_v4();
    s2.title = "Second Scenario".into();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&s1).unwrap()).await;
    send_json(&router, "POST", "/scenarios", serde_json::to_value(&s2).unwrap()).await;

    let (status, body) = send_empty(&router, "GET", "/scenarios").await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn update_scenario_changes_title() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();
    let id = scenario.id;

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;

    let mut updated = scenario.clone();
    updated.title = "Updated Title".into();
    let (status, body) = send_json(
        &router,
        "PUT",
        &format!("/scenarios/{id}"),
        serde_json::to_value(&updated).unwrap(),
    )
    .await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["title"], "Updated Title");
}

#[tokio::test]
async fn delete_scenario_removes_it() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();
    let id = scenario.id;

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (delete_status, _) = send_empty(&router, "DELETE", &format!("/scenarios/{id}")).await;
    assert_eq!(delete_status, StatusCode::OK);

    let (get_status, _) = send_empty(&router, "GET", &format!("/scenarios/{id}")).await;
    assert_eq!(get_status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_unknown_scenario_returns_404() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let random_id = Uuid::new_v4();

    let (status, _) = send_empty(&router, "GET", &format!("/scenarios/{random_id}")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Session lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_session_with_unknown_scenario_returns_404() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let random_id = Uuid::new_v4();

    let (status, _) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": random_id, "title": "Test Session" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_session_returns_session_record() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (status, body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Guildhall Trial" }),
    )
    .await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert!(payload.get("id").is_some());
    assert_eq!(payload["scenario_id"], scenario.id.to_string());
}

#[tokio::test]
async fn get_session_returns_created_session() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "My Session" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let (status, body) = send_empty(&router, "GET", &format!("/sessions/{session_id}")).await;
    let fetched: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["id"], session["id"]);
}

#[tokio::test]
async fn delete_session_removes_it() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Delete Me" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let (delete_status, _) = send_empty(&router, "DELETE", &format!("/sessions/{session_id}")).await;
    assert_eq!(delete_status, StatusCode::OK);

    let (get_status, _) = send_empty(&router, "GET", &format!("/sessions/{session_id}")).await;
    assert_eq!(get_status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_sessions_returns_all() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Session One" }),
    )
    .await;
    send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Session Two" }),
    )
    .await;

    let (status, body) = send_empty(&router, "GET", "/sessions").await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload.as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// Provider management
// ---------------------------------------------------------------------------

#[tokio::test]
async fn register_provider_returns_created_record() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));

    let (status, body) = send_json(
        &router,
        "POST",
        "/providers",
        json!({
            "name": "test-provider",
            "provider_type": "openai_compatible",
            "base_url": "http://localhost:11434",
            "model": "llama3",
            "api_key_secret_ref": null,
            "capabilities": null,
            "is_default": false
        }),
    )
    .await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(payload["name"], "test-provider");
    assert!(payload.get("id").is_some());
}

#[tokio::test]
async fn list_providers_returns_all_registered() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));

    for name in ["provider-a", "provider-b"] {
        send_json(
            &router,
            "POST",
            "/providers",
            json!({
                "name": name,
                "provider_type": "openai_compatible",
                "base_url": "http://localhost:11434",
                "model": "llama3",
                "api_key_secret_ref": null,
                "capabilities": null,
                "is_default": false
            }),
        )
        .await;
    }

    let (status, body) = send_empty(&router, "GET", "/providers").await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn delete_provider_removes_it() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));

    let (_, create_body) = send_json(
        &router,
        "POST",
        "/providers",
        json!({
            "name": "to-delete",
            "provider_type": "openai_compatible",
            "base_url": "http://localhost:11434",
            "model": "llama3",
            "api_key_secret_ref": null,
            "capabilities": null,
            "is_default": false
        }),
    )
    .await;
    let created: Value = json_body(&create_body);
    let provider_id = created["id"].as_str().unwrap();

    send_empty(&router, "DELETE", &format!("/providers/{provider_id}")).await;

    let (_, list_body) = send_empty(&router, "GET", "/providers").await;
    let list: Value = json_body(&list_body);

    assert_eq!(list.as_array().unwrap().len(), 0);
}

// ---------------------------------------------------------------------------
// Session-provider assignment
// ---------------------------------------------------------------------------

#[tokio::test]
async fn set_session_provider_persists_provider_id() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Provider Test" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let provider_id = Uuid::new_v4();
    let (status, body) = send_json(
        &router,
        "PATCH",
        &format!("/sessions/{session_id}/provider"),
        json!({ "provider_id": provider_id }),
    )
    .await;
    let updated: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["provider_id"], provider_id.to_string());
}

#[tokio::test]
async fn clear_session_provider_with_null() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Clear Provider Test" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    // First set a provider, then clear it.
    let provider_id = Uuid::new_v4();
    send_json(
        &router,
        "PATCH",
        &format!("/sessions/{session_id}/provider"),
        json!({ "provider_id": provider_id }),
    )
    .await;

    let (status, body) = send_json(
        &router,
        "PATCH",
        &format!("/sessions/{session_id}/provider"),
        json!({ "provider_id": null }),
    )
    .await;
    let updated: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert!(updated["provider_id"].is_null());
}

// ---------------------------------------------------------------------------
// Turn validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn turn_on_missing_session_returns_404() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let random_id = Uuid::new_v4();

    let (status, _) = send_json(
        &router,
        "POST",
        &format!("/sessions/{random_id}/turn"),
        json!({ "input": "hello", "mode": "action" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn world_state_on_fresh_session_has_version_zero() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Fresh Session" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let (status, body) =
        send_empty(&router, "GET", &format!("/sessions/{session_id}/world-state")).await;
    let world_state: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(world_state["state_version"], 0);
}

// ---------------------------------------------------------------------------
// Full in-memory turn cycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn in_memory_turn_cycle_applies_delta_and_returns_response() {
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
    let router = memory_test_context(mock_provider([raw.to_string()]));
    let scenario = sample_scenario();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Guildhall Trial" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let (status, body) = send_json(
        &router,
        "POST",
        &format!("/sessions/{session_id}/turn"),
        json!({ "input": "I flood the guildhall", "mode": "action" }),
    )
    .await;
    let response: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["world_state_version"], 1);
    let player_response = response["player_response"].as_str().unwrap_or("");
    assert!(!player_response.is_empty(), "player_response must be a non-empty string");
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_empty_on_fresh_session() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(&router, "POST", "/scenarios", serde_json::to_value(&scenario).unwrap()).await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Events Test" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let (status, body) =
        send_empty(&router, "GET", &format!("/sessions/{session_id}/events")).await;
    let events: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(events.as_array().unwrap().len(), 0);
}
