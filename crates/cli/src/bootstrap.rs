//! CLI state assembly.
//!
//! Mirrors [`api::AppState::new`] / `new_memory` but without the HTTP-only
//! provider registry — the CLI is a single-process, single-user runner.

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use domain::SessionId;
use engine::{InMemorySessionTurnLock, SessionTurnLock};
use persistence::{ApplicationStore, InMemoryApplicationStore, PgPersistence, PostgresSessionTurnLock, ProviderRecord};
use providers::{LlmProvider, build_provider_from_config, build_provider_from_record_fields};
use uuid::Uuid;
use shared::{AppConfig, StorageBackend};

pub struct CliRuntimeOptions {
    pub use_postgres: bool,
    pub config_path: Option<String>,
}

pub struct CliState {
    pub config: AppConfig,
    pub store: Arc<dyn ApplicationStore>,
    pub provider: Arc<dyn LlmProvider>,
    pub turn_lock: Arc<dyn SessionTurnLock>,
}

impl CliState {
    pub async fn provider_record(&self, provider_id: Uuid) -> Result<Option<ProviderRecord>> {
        Ok(self
            .store
            .list_providers()
            .await?
            .into_iter()
            .find(|record| record.id == provider_id))
    }

    pub fn build_provider_from_record(
        &self,
        record: &ProviderRecord,
    ) -> Result<Arc<dyn LlmProvider>> {
        build_provider_from_record_fields(
            record.name.clone(),
            record.provider_type.clone(),
            record.base_url.clone(),
            record.api_key_secret_ref.clone(),
            record.model.clone(),
            record.capabilities.clone(),
        )
    }

    pub async fn resolve_provider_by_id(&self, provider_id: Uuid) -> Result<Arc<dyn LlmProvider>> {
        let record = self
            .provider_record(provider_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("configured session provider {provider_id} is not available"))?;
        self.build_provider_from_record(&record)
    }

    pub async fn resolve_session_provider(
        &self,
        session_id: SessionId,
    ) -> Result<Arc<dyn LlmProvider>> {
        let session = self
            .store
            .get_session(session_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("session {session_id} not found"))?;
        match session.provider_id {
            Some(provider_id) => self.resolve_provider_by_id(provider_id).await,
            None => Ok(Arc::clone(&self.provider)),
        }
    }
}

pub async fn build_state(opts: CliRuntimeOptions) -> Result<CliState> {
    let config_path = opts.config_path.as_deref().map(PathBuf::from);
    let mut config = AppConfig::load(config_path.as_deref())?;
    // The CLI defaults to in-memory storage so casual playtesting doesn't
    // require a running Postgres. Pass `--postgres` (or set
    // ROLEPLAY_CLI_POSTGRES=1) to opt into persistent storage.
    config.storage.backend = if opts.use_postgres {
        StorageBackend::Postgres
    } else {
        StorageBackend::Memory
    };
    config.validate()?;
    let provider = build_provider_from_config(&config.provider.default)?;

    let (store, turn_lock): (Arc<dyn ApplicationStore>, Arc<dyn SessionTurnLock>) =
        match config.storage.backend {
            StorageBackend::Memory => (
                Arc::new(InMemoryApplicationStore::new(
                    config.debug.store_raw_provider_output,
                )),
                Arc::new(InMemorySessionTurnLock::default()),
            ),
            StorageBackend::Postgres => {
                let persistence = PgPersistence::connect(&config.database.url).await?;
                if config.storage.migrate_on_startup {
                    persistence.migrate().await?;
                }
                let lock = Arc::new(PostgresSessionTurnLock::new(persistence.pool().clone()));
                let store = Arc::new(persistence::PostgresApplicationStore::new(
                    persistence,
                    config.debug.store_raw_provider_output,
                ));
                (store, lock)
            }
        };

    Ok(CliState {
        config,
        store,
        provider,
        turn_lock,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Location, Scenario, ScenarioType};
    use engine::InMemorySessionTurnLock;
    use persistence::ProviderRecord;
    use serde_json::json;
    use shared::ProviderConfig;

    fn provider_record(
        id: Uuid,
        name: &str,
        base_url: &str,
    ) -> ProviderRecord {
        ProviderRecord {
            id,
            name: name.into(),
            provider_type: "openai_compatible".into(),
            base_url: base_url.into(),
            model: "test-model".into(),
            api_key_secret_ref: None,
            capabilities: json!({
                "supports_streaming": true,
                "supports_json_mode": true,
                "supports_tool_calls": false,
                "supports_seed": false,
                "max_context_tokens": null,
                "request_timeout_seconds": 5,
                "stream_idle_timeout_seconds": 5,
                "max_retries": 0
            }),
            is_default: false,
        }
    }

    fn provider_config(name: &str, base_url: &str) -> ProviderConfig {
        ProviderConfig {
            name: name.into(),
            provider_type: "openai_compatible".into(),
            base_url: base_url.into(),
            api_key: None,
            model: "default-model".into(),
            supports_streaming: true,
            supports_json_mode: true,
            max_context_tokens: None,
            request_timeout_seconds: 5,
            stream_idle_timeout_seconds: 5,
            max_retries: 0,
            http_referer: None,
            x_title: None,
            provider_routing: None,
            include_usage: true,
        }
    }

    fn scenario() -> Scenario {
        Scenario {
            id: Uuid::new_v4(),
            title: "Provider Override".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "test".into(),
            tone: "test".into(),
            rules: vec![],
            locations: vec![Location {
                id: "guildhall".into(),
                name: "Guildhall".into(),
                description: "The opening hall.".into(),
                visible: true,
            }],
            factions: vec![],
            npcs: vec![],
            quests: vec![],
            secrets: vec![],
            clocks: vec![],
        }
    }

    #[tokio::test]
    async fn resolve_session_provider_returns_override_provider_when_session_has_provider_id() {
        let store = Arc::new(InMemoryApplicationStore::new(false));
        let scenario = store.insert_scenario(scenario());
        let session = store
            .insert_session(scenario.id, "Provider Override".into())
            .expect("session");
        let provider_id = Uuid::new_v4();
        store
            .create_provider(provider_record(provider_id, "override-provider", ""))
            .await
            .expect("provider record");
        store
            .set_session_provider(session.id, Some(provider_id))
            .await
            .expect("set provider")
            .expect("session should exist");

        let mut config = AppConfig::default();
        config.storage.backend = StorageBackend::Memory;
        config.provider.default = provider_config("default-provider", "");
        let state = CliState {
            config,
            store,
            provider: build_provider_from_config(&provider_config("default-provider", ""))
                .expect("default provider"),
            turn_lock: Arc::new(InMemorySessionTurnLock::default()),
        };

        let resolved = state
            .resolve_session_provider(session.id)
            .await
            .expect("resolved provider");
        let health = resolved.health().await.expect("health");

        assert_eq!(health.name, "override-provider");
    }

    #[tokio::test]
    async fn resolve_provider_by_id_errors_when_record_is_missing() {
        let mut config = AppConfig::default();
        config.storage.backend = StorageBackend::Memory;
        config.provider.default = provider_config("default-provider", "");
        let state = CliState {
            config,
            store: Arc::new(InMemoryApplicationStore::new(false)),
            provider: build_provider_from_config(&provider_config("default-provider", ""))
                .expect("default provider"),
            turn_lock: Arc::new(InMemorySessionTurnLock::default()),
        };
        let missing_id = Uuid::new_v4();

        let error = match state.resolve_provider_by_id(missing_id).await {
            Ok(_) => panic!("missing provider should error"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains(&format!("configured session provider {missing_id} is not available"))
        );
    }
}
