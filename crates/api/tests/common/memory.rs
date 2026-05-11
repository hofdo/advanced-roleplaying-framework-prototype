use api::{ApiStore, AppState, app_router};
use axum::Router;
use engine::InMemorySessionTurnLock;
use providers::LlmProvider;
use std::sync::Arc;

pub fn memory_test_context(provider: Arc<dyn LlmProvider>) -> Router {
    memory_test_context_with_config(provider, {
        let mut config = shared::AppConfig::default();
        config.storage.backend = shared::StorageBackend::Memory;
        config
    })
}

pub fn memory_test_context_with_config(
    provider: Arc<dyn LlmProvider>,
    config: shared::AppConfig,
) -> Router {
    let store_raw_provider_output = config.debug.store_raw_provider_output;
    let state = AppState::from_parts(
        config,
        Arc::new(ApiStore::new(store_raw_provider_output)),
        provider,
        Arc::new(InMemorySessionTurnLock::default()),
    );
    app_router(state)
}
