//! CLI state assembly.
//!
//! Mirrors [`api::AppState::new`] / `new_memory` but without the HTTP-only
//! provider registry — the CLI is a single-process, single-user runner.

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use engine::{InMemorySessionTurnLock, SessionTurnLock};
use persistence::{
    ApplicationStore, InMemoryApplicationStore, PgPersistence, PostgresSessionTurnLock,
};
use providers::{LlmProvider, build_provider_from_config};
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
