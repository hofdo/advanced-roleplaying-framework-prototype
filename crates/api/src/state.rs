use domain::{FrontendVisibleState, SessionId, ViewerContext};
use engine::{
    FrontendStateProjector, InMemorySessionTurnLock, SessionTurnLock, TurnPipelineError,
};
use persistence::{
    PgPersistence, PostgresSessionTurnLock, ProviderConfigRepository, ProviderRecord,
};
use providers::{
    LlamaCppProvider, LlmProvider, OpenAiCompatibleProvider, OpenRouterExtras, OpenRouterProvider,
    ProviderCapabilities, resolve_secret,
};
use shared::{AppConfig, StorageBackend};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

pub use persistence::{ApiStore, ApplicationStore, PostgresApplicationStore};
pub use providers::build_provider_from_config;

#[derive(Debug, Error)]
pub enum ResolveProviderError {
    #[error("configured session provider {0} is not available")]
    Missing(Uuid),
}

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub store: Arc<dyn ApplicationStore>,
    pub provider: Arc<dyn LlmProvider>,
    pub provider_registry: Arc<RwLock<HashMap<Uuid, Arc<dyn LlmProvider>>>>,
    pub turn_lock: Arc<dyn SessionTurnLock>,
}

impl AppState {
    pub async fn new(config: AppConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let provider = build_provider_from_config(&config.provider.default)?;

        let (store, turn_lock, provider_registry): (
            Arc<dyn ApplicationStore>,
            Arc<dyn SessionTurnLock>,
            Arc<RwLock<HashMap<Uuid, Arc<dyn LlmProvider>>>>,
        ) = match config.storage.backend {
            StorageBackend::Memory => (
                Arc::new(ApiStore::new(config.debug.store_raw_provider_output)),
                Arc::new(InMemorySessionTurnLock::default()),
                Arc::new(RwLock::new(HashMap::new())),
            ),
            StorageBackend::Postgres => {
                let persistence = PgPersistence::connect(&config.database.url).await?;
                if config.storage.migrate_on_startup {
                    persistence.migrate().await?;
                }
                let pg_lock = Arc::new(PostgresSessionTurnLock::new(persistence.pool().clone()));
                let db_records = ProviderConfigRepository::list(&persistence).await?;
                let registry = build_provider_registry(&db_records)?;
                (
                    Arc::new(PostgresApplicationStore::new(
                        persistence,
                        config.debug.store_raw_provider_output,
                    )),
                    pg_lock,
                    Arc::new(RwLock::new(registry)),
                )
            }
        };

        Ok(Self {
            config,
            store,
            provider,
            provider_registry,
            turn_lock,
        })
    }

    pub fn new_memory(config: AppConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let store_raw_provider_output = config.debug.store_raw_provider_output;
        let provider = build_provider_from_config(&config.provider.default)?;

        Ok(Self {
            config,
            store: Arc::new(ApiStore::new(store_raw_provider_output)),
            provider,
            provider_registry: Arc::new(RwLock::new(HashMap::new())),
            turn_lock: Arc::new(InMemorySessionTurnLock::default()),
        })
    }

    pub fn from_parts(
        config: AppConfig,
        store: Arc<dyn ApplicationStore>,
        provider: Arc<dyn LlmProvider>,
        turn_lock: Arc<dyn SessionTurnLock>,
    ) -> Self {
        config
            .validate()
            .expect("invalid admin configuration for application state");
        Self {
            config,
            store,
            provider,
            provider_registry: Arc::new(RwLock::new(HashMap::new())),
            turn_lock,
        }
    }

    pub async fn resolve_provider(
        &self,
        provider_id: Option<Uuid>,
    ) -> Result<Arc<dyn LlmProvider>, ResolveProviderError> {
        if let Some(id) = provider_id {
            let registry = self.provider_registry.read().await;
            if let Some(p) = registry.get(&id) {
                return Ok(Arc::clone(p));
            }
            return Err(ResolveProviderError::Missing(id));
        }
        Ok(Arc::clone(&self.provider))
    }
}

pub fn build_provider_registry(
    records: &[ProviderRecord],
) -> anyhow::Result<HashMap<Uuid, Arc<dyn LlmProvider>>> {
    let mut registry = HashMap::new();
    for record in records {
        let provider = provider_from_record(record).map_err(|error| {
            anyhow::anyhow!("invalid provider {} ({}): {error}", record.id, record.name)
        })?;
        registry.insert(record.id, provider);
    }
    Ok(registry)
}

pub fn provider_from_record(record: &ProviderRecord) -> anyhow::Result<Arc<dyn LlmProvider>> {
    let caps: ProviderCapabilities = serde_json::from_value(record.capabilities.clone())
        .map_err(|e| anyhow::anyhow!("invalid provider capabilities: {e}"))?;
    let api_key = resolve_secret(record.api_key_secret_ref.as_deref())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    match record.provider_type.as_str() {
        "" | "openai_compatible" => Ok(Arc::new(
            OpenAiCompatibleProvider::new(
                record.name.clone(),
                record.base_url.clone(),
                api_key,
                record.model.clone(),
                caps,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        )),
        "llama_cpp" => Ok(Arc::new(
            LlamaCppProvider::new(
                record.name.clone(),
                record.base_url.clone(),
                api_key,
                record.model.clone(),
                caps,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        )),
        "openrouter" => {
            let extras: OpenRouterExtras =
                serde_json::from_value(record.capabilities.clone()).unwrap_or_default();
            Ok(Arc::new(
                OpenRouterProvider::new(
                    record.base_url.clone(),
                    api_key,
                    record.model.clone(),
                    caps,
                    extras,
                )
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
            ))
        }
        other => anyhow::bail!("unknown provider_type '{other}'"),
    }
}

pub async fn project_session_state(
    state: &AppState,
    session_id: SessionId,
) -> Result<Option<FrontendVisibleState>, TurnPipelineError> {
    let Some(session) = state.store.get_session(session_id).await? else {
        return Ok(None);
    };
    let Some(scenario) = state.store.get_scenario(session.scenario_id).await? else {
        return Ok(None);
    };
    let Some(world_state) = state.store.world_state(session_id).await? else {
        return Ok(None);
    };
    Ok(Some(engine::BasicFrontendStateProjector.project(
        &scenario,
        &world_state,
        &ViewerContext::player(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use providers::MockProvider;

    #[tokio::test]
    async fn memory_app_state_reports_memory_storage_status() {
        let mut config = AppConfig::default();
        config.storage.backend = StorageBackend::Memory;

        let state = AppState::new_memory(config).expect("memory app state");

        assert_eq!(state.store.storage_status().await, "memory");
    }

    #[tokio::test]
    async fn resolve_provider_returns_registry_entry_when_id_matches() {
        let default_provider: Arc<dyn LlmProvider> =
            Arc::new(MockProvider::new("default", std::iter::empty::<String>()));
        let registry_provider: Arc<dyn LlmProvider> =
            Arc::new(MockProvider::new("registry", std::iter::empty::<String>()));
        let registry_id = Uuid::new_v4();

        let mut config = AppConfig::default();
        config.storage.backend = StorageBackend::Memory;
        let state = AppState::from_parts(
            config,
            Arc::new(ApiStore::new(false)),
            Arc::clone(&default_provider),
            Arc::new(InMemorySessionTurnLock::default()),
        );
        state
            .provider_registry
            .write()
            .await
            .insert(registry_id, Arc::clone(&registry_provider));

        let resolved = state.resolve_provider(Some(registry_id)).await.unwrap();
        let resolved_name = resolved.health().await.unwrap().name;

        assert_eq!(resolved_name, "registry");
    }

    #[tokio::test]
    async fn resolve_provider_returns_error_when_id_not_in_registry() {
        let default_provider: Arc<dyn LlmProvider> =
            Arc::new(MockProvider::new("default", std::iter::empty::<String>()));
        let unknown_id = Uuid::new_v4();

        let mut config = AppConfig::default();
        config.storage.backend = StorageBackend::Memory;
        let state = AppState::from_parts(
            config,
            Arc::new(ApiStore::new(false)),
            Arc::clone(&default_provider),
            Arc::new(InMemorySessionTurnLock::default()),
        );

        let error = match state.resolve_provider(Some(unknown_id)).await {
            Ok(_) => panic!("missing provider should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("provider"));
    }

    #[tokio::test]
    async fn resolve_provider_returns_default_when_no_provider_id() {
        let default_provider: Arc<dyn LlmProvider> =
            Arc::new(MockProvider::new("default", std::iter::empty::<String>()));

        let mut config = AppConfig::default();
        config.storage.backend = StorageBackend::Memory;
        let state = AppState::from_parts(
            config,
            Arc::new(ApiStore::new(false)),
            Arc::clone(&default_provider),
            Arc::new(InMemorySessionTurnLock::default()),
        );

        let resolved = state.resolve_provider(None).await.unwrap();
        let resolved_name = resolved.health().await.unwrap().name;

        assert_eq!(resolved_name, "default");
    }
}
