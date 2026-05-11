mod common;

use api::build_provider_from_config;
use common::{json_body, postgres_test_context_with_config, send_empty, send_json};
use http::StatusCode;
use providers::{LlmProvider, ProviderModel};
use serde_json::{Value, json};
use std::{env, sync::Arc};
use uuid::Uuid;

fn live_llama_provider_config() -> shared::ProviderConfig {
    shared::ProviderConfig {
        name: "local-llama-live".into(),
        provider_type: env::var("TEST_LLM_PROVIDER_TYPE").unwrap_or_else(|_| "llama_cpp".into()),
        base_url: env::var("TEST_LLM_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".into()),
        api_key: env::var("TEST_LLM_API_KEY").ok(),
        model: env::var("TEST_LLM_MODEL").unwrap_or_else(|_| "local-model".into()),
        supports_streaming: true,
        supports_json_mode: false,
        max_context_tokens: Some(32_768),
        request_timeout_seconds: 120,
        stream_idle_timeout_seconds: 30,
        max_retries: 0,
        http_referer: None,
        x_title: None,
        provider_routing: None,
        include_usage: true,
    }
}

fn live_llama_provider() -> Arc<dyn LlmProvider> {
    Arc::clone(
        &build_provider_from_config(&live_llama_provider_config()).expect("build live provider"),
    )
}

fn postgres_live_config() -> shared::AppConfig {
    let mut config = shared::AppConfig::default();
    config.storage.backend = shared::StorageBackend::Postgres;
    config.provider.default = live_llama_provider_config();
    config
}

#[tokio::test]
#[ignore = "manual smoke test requiring docker daemon and local llama-server"]
async fn live_stack_reports_postgres_and_llama_health() {
    let ctx = postgres_test_context_with_config(live_llama_provider(), postgres_live_config())
        .await
        .expect("test context");

    let (status, body) = send_empty(&ctx.router, "GET", "/health").await;
    let health: Value = json_body(&body);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["status"], "ok");
    assert_eq!(health["database"], "postgres:ok");
    assert_eq!(health["active_provider"], "local-llama-live");

    let (status, body) = send_empty(&ctx.router, "GET", "/providers/health").await;
    let provider_health: Value = json_body(&body);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(provider_health["name"], "local-llama-live");
    assert_eq!(provider_health["ok"], true);

    let (status, body) = send_empty(&ctx.router, "GET", "/providers/readiness").await;
    let readiness: Value = json_body(&body);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(readiness["configured"], true);
    assert_eq!(readiness["reachable"], true);

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore = "manual smoke test requiring docker daemon and local llama-server"]
async fn live_stack_can_persist_provider_and_list_models() {
    let ctx = postgres_test_context_with_config(live_llama_provider(), postgres_live_config())
        .await
        .expect("test context");

    let provider_config = live_llama_provider_config();
    let provider_name = format!("llama-live-{}", Uuid::new_v4());
    let (status, body) = send_json(
        &ctx.router,
        "POST",
        "/providers",
        json!({
            "name": provider_name,
            "provider_type": provider_config.provider_type,
            "base_url": provider_config.base_url,
            "model": provider_config.model,
            "api_key_secret_ref": provider_config.api_key,
            "capabilities": {
                "supports_streaming": true,
                "supports_model_listing": true
            },
            "is_default": false
        }),
    )
    .await;
    let created: Value = json_body(&body);

    assert_eq!(status, StatusCode::CREATED);
    let provider_id = created["id"].as_str().expect("provider id");

    let (status, body) = send_empty(
        &ctx.router,
        "GET",
        &format!("/providers/{provider_id}/models"),
    )
    .await;
    let models: Vec<ProviderModel> = json_body(&body);

    assert_eq!(status, StatusCode::OK);
    assert!(
        !models.is_empty(),
        "expected at least one model from local llama-server"
    );

    ctx.cleanup().await;
}
