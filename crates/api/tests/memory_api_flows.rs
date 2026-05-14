mod common;
#[path = "common/memory.rs"]
mod common_memory;

use api::{ApiStore, AppState, app_router};
use async_trait::async_trait;
use common::{
    joined_request_text, json_body, mock_provider, recording_mock_provider, sample_scenario,
    send_empty, send_empty_with_bearer, send_json, turn_responses,
};
use common_memory::{memory_test_context, memory_test_context_with_config};
use engine::InMemorySessionTurnLock;
use http::StatusCode;
use providers::{
    LlmProvider, LlmRequest, LlmResponse, ProviderCapabilities, ProviderHealth, ProviderModel,
    ProviderReadiness, TokenStream,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::time::{Duration, timeout};
use uuid::Uuid;

const ADMIN_TOKEN: &str = "test-admin-token";

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

#[tokio::test]
async fn admin_routes_return_404_when_disabled() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));

    let (status, _) = send_empty(
        &router,
        "GET",
        "/admin/sessions/00000000-0000-0000-0000-000000000000/export/raw",
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_routes_require_bearer_token_when_enabled() {
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Memory;
    config.admin.enabled = true;
    config.admin.token = Some(ADMIN_TOKEN.into());
    let router = memory_test_context_with_config(mock_provider(Vec::<String>::new()), config);

    let path = "/admin/sessions/00000000-0000-0000-0000-000000000000/export/raw";
    let (missing_status, _) = send_empty(&router, "GET", path).await;
    let (wrong_status, _) = send_empty_with_bearer(&router, "GET", path, "wrong-token").await;
    let (correct_status, _) = send_empty_with_bearer(&router, "GET", path, ADMIN_TOKEN).await;

    assert_eq!(missing_status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_status, StatusCode::UNAUTHORIZED);
    assert_eq!(correct_status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Scenario CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_scenario_returns_created_scenario() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();
    let id = scenario.id;

    let (status, body) = send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&s1).unwrap(),
    )
    .await;
    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&s2).unwrap(),
    )
    .await;

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

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;

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

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Delete Me" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let (delete_status, _) =
        send_empty(&router, "DELETE", &format!("/sessions/{session_id}")).await;
    assert_eq!(delete_status, StatusCode::OK);

    let (get_status, _) = send_empty(&router, "GET", &format!("/sessions/{session_id}")).await;
    assert_eq!(get_status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_sessions_returns_all() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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
async fn register_provider_rejects_unknown_provider_type() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));

    let (status, body) = send_json(
        &router,
        "POST",
        "/providers",
        json!({
            "name": "bad-provider",
            "provider_type": "unknown_provider_xyz",
            "base_url": "http://localhost:11434",
            "model": "llama3",
            "api_key_secret_ref": null,
            "capabilities": null,
            "is_default": false
        }),
    )
    .await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        payload["error"]
            .as_str()
            .unwrap()
            .contains("unknown provider_type")
    );

    let (_, list_body) = send_empty(&router, "GET", "/providers").await;
    let list: Value = json_body(&list_body);
    assert!(list.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn register_provider_rejects_missing_env_secret() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    unsafe { std::env::remove_var("TEST_PROVIDER_SECRET_MISSING") };

    let (status, body) = send_json(
        &router,
        "POST",
        "/providers",
        json!({
            "name": "bad-secret-provider",
            "provider_type": "openai_compatible",
            "base_url": "http://localhost:11434",
            "model": "llama3",
            "api_key_secret_ref": "env:TEST_PROVIDER_SECRET_MISSING",
            "capabilities": null,
            "is_default": false
        }),
    )
    .await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        payload["error"]
            .as_str()
            .unwrap()
            .contains("TEST_PROVIDER_SECRET_MISSING")
    );

    let (_, list_body) = send_empty(&router, "GET", "/providers").await;
    let list: Value = json_body(&list_body);
    assert!(list.as_array().unwrap().is_empty());
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

#[tokio::test]
async fn delete_provider_clears_matching_session_provider_assignment() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Assigned Provider Session" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

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

    let (status, body) = send_json(
        &router,
        "PATCH",
        &format!("/sessions/{session_id}/provider"),
        json!({ "provider_id": provider_id }),
    )
    .await;
    let updated: Value = json_body(&body);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["provider_id"], provider_id);

    let (delete_status, _) =
        send_empty(&router, "DELETE", &format!("/providers/{provider_id}")).await;
    assert_eq!(delete_status, StatusCode::OK);

    let (_, session_body) = send_empty(&router, "GET", &format!("/sessions/{session_id}")).await;
    let session_after_delete: Value = json_body(&session_body);
    assert!(session_after_delete["provider_id"].is_null());
}

#[tokio::test]
async fn list_models_returns_501_for_openai_compatible_provider() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));

    let (_, create_body) = send_json(
        &router,
        "POST",
        "/providers",
        json!({
            "name": "models-test-provider",
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

    let (status, _) = send_empty(&router, "GET", &format!("/providers/{provider_id}/models")).await;

    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn list_models_returns_404_for_unknown_provider() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let unknown_id = Uuid::new_v4();

    let (status, _) = send_empty(&router, "GET", &format!("/providers/{unknown_id}/models")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[derive(Debug)]
struct SlowModelsProvider;

#[async_trait]
impl LlmProvider for SlowModelsProvider {
    async fn health(&self) -> Result<ProviderHealth, providers::ProviderError> {
        Ok(ProviderHealth {
            name: "slow".into(),
            ok: true,
            message: None,
        })
    }

    async fn readiness(&self) -> Result<ProviderReadiness, providers::ProviderError> {
        Ok(ProviderReadiness {
            configured: true,
            reachable: true,
            message: "slow".into(),
        })
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_model_listing: true,
            ..ProviderCapabilities::default()
        }
    }

    async fn generate(
        &self,
        _request: LlmRequest,
    ) -> Result<LlmResponse, providers::ProviderError> {
        Ok(LlmResponse {
            text: "unused".into(),
            raw_json: None,
            usage: None,
            cost_usd: None,
            generation_id: None,
        })
    }

    async fn stream(&self, _request: LlmRequest) -> Result<TokenStream, providers::ProviderError> {
        Err(providers::ProviderError::StreamingUnsupported)
    }

    async fn list_models(&self) -> Result<Vec<ProviderModel>, providers::ProviderError> {
        tokio::time::sleep(Duration::from_millis(250)).await;
        Ok(vec![ProviderModel {
            id: "slow-model".into(),
            name: "Slow Model".into(),
            context_length: None,
            pricing: None,
        }])
    }
}

#[tokio::test]
async fn list_models_does_not_block_registry_write_lock() {
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Memory;
    let default_provider = mock_provider(Vec::<String>::new());
    let state = AppState::from_parts(
        config,
        Arc::new(ApiStore::new(false)),
        Arc::clone(&default_provider),
        Arc::new(InMemorySessionTurnLock::default()),
    );
    let provider_id = Uuid::new_v4();
    state
        .provider_registry
        .write()
        .await
        .insert(provider_id, Arc::new(SlowModelsProvider));
    let router = app_router(state);

    let slow_router = router.clone();
    let slow_request = tokio::spawn(async move {
        send_empty(
            &slow_router,
            "GET",
            &format!("/providers/{provider_id}/models"),
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(25)).await;

    let fast_request = timeout(
        Duration::from_millis(100),
        send_json(
            &router,
            "POST",
            "/providers",
            json!({
                "name": "fast-provider",
                "provider_type": "openai_compatible",
                "base_url": "http://localhost:11434",
                "model": "llama3",
                "api_key_secret_ref": null,
                "capabilities": null,
                "is_default": false
            }),
        ),
    )
    .await;

    assert!(
        fast_request.is_ok(),
        "provider registration was blocked by list_models"
    );

    let (slow_status, _) = slow_request.await.expect("slow request task");
    assert_eq!(slow_status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Session-provider assignment
// ---------------------------------------------------------------------------

#[tokio::test]
async fn set_session_provider_persists_provider_id() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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
async fn turn_with_missing_session_provider_returns_409() {
    let router = memory_test_context(mock_provider([
        r#"{"player_response":"The room stills.","world_state_delta":{"facts_to_add":[],"npc_changes":[],"faction_changes":[],"quest_changes":[],"clock_changes":[],"relationship_changes":[],"location_change":null,"event_log_entries":[]}}"#.to_owned()
    ]));
    let scenario = sample_scenario();

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Missing Provider Turn" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let missing_provider_id = Uuid::new_v4();
    send_json(
        &router,
        "PATCH",
        &format!("/sessions/{session_id}/provider"),
        json!({ "provider_id": missing_provider_id }),
    )
    .await;

    let (status, body) = send_json(
        &router,
        "POST",
        &format!("/sessions/{session_id}/turn"),
        json!({ "input": "hello", "mode": "action" }),
    )
    .await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(payload["error"].as_str().unwrap().contains("provider"));
}

#[tokio::test]
async fn world_state_on_fresh_session_has_version_zero() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Fresh Session" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let (status, body) = send_empty(
        &router,
        "GET",
        &format!("/sessions/{session_id}/world-state"),
    )
    .await;
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
    let router = memory_test_context(mock_provider(turn_responses([raw.to_string()])));
    let scenario = sample_scenario();

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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
    assert!(
        !player_response.is_empty(),
        "player_response must be a non-empty string"
    );
}

#[tokio::test]
async fn non_streaming_turn_visible_prompt_does_not_receive_gm_only_fact() {
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
    let router = memory_test_context(provider);
    let scenario = sample_scenario();

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
    let (_, session_body) = send_json(
        &router,
        "POST",
        "/sessions",
        json!({ "scenario_id": scenario.id, "title": "Secrecy Boundary" }),
    )
    .await;
    let session: Value = json_body(&session_body);
    let session_id = session["id"].as_str().unwrap();

    let (status, body) = send_json(
        &router,
        "POST",
        &format!("/sessions/{session_id}/turn"),
        json!({
            "input": "I ask the examiner what the soul-mark really means.",
            "mode": "action",
        }),
    )
    .await;
    let payload: Value = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert!(payload.get("message_id").is_some());
    assert_eq!(payload["world_state_version"], 1);
    assert!(payload.get("frontend_state_patch").is_some());
    assert!(payload.get("raw_provider_output").is_none());

    let requests = recorded_requests.lock().expect("recorded requests");
    assert_eq!(
        requests.len(),
        2,
        "non-streaming turn must call provider twice"
    );
    assert!(
        !joined_request_text(&requests[0]).contains("soul-mark was not created"),
        "first (visible) request must not include GM-only fact text"
    );
    assert!(
        joined_request_text(&requests[1]).contains("soul-mark was not created"),
        "second (delta-extraction) request must include GM-only fact text"
    );
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_empty_on_fresh_session() {
    let router = memory_test_context(mock_provider(Vec::<String>::new()));
    let scenario = sample_scenario();

    send_json(
        &router,
        "POST",
        "/scenarios",
        serde_json::to_value(&scenario).unwrap(),
    )
    .await;
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
