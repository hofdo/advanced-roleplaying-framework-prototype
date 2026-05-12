use anyhow::{Context, Result};
use clap::Subcommand;
use persistence::ProviderRecord;
use uuid::Uuid;

use crate::{bootstrap::CliState, render::print_json};

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Register a provider configuration (Postgres only).
    Register {
        /// Path to a ProviderRecord JSON file.
        #[arg(long)]
        file: String,
    },
    /// List all registered providers.
    List,
    /// Remove a provider by id.
    Remove { provider_id: Uuid },
    /// List models exposed by a provider's backend.
    Models { provider_id: Uuid },
}

pub async fn run(state: CliState, cmd: Cmd) -> Result<()> {
    if matches!(state.config.storage.backend, shared::StorageBackend::Memory) {
        anyhow::bail!("provider management requires --postgres");
    }

    match cmd {
        Cmd::Register { file } => {
            let bytes = std::fs::read(&file).with_context(|| format!("reading {file}"))?;
            let record: ProviderRecord = serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing provider record {file}"))?;
            let saved = state.store.create_provider(record).await?;
            print_json(&saved)
        }
        Cmd::List => {
            let records = state.store.list_providers().await?;
            print_json(&records)
        }
        Cmd::Remove { provider_id } => {
            state.store.delete_provider(provider_id).await?;
            println!("removed provider {provider_id}");
            Ok(())
        }
        Cmd::Models { provider_id } => {
            let records = state.store.list_providers().await?;
            let record = records
                .iter()
                .find(|r| r.id == provider_id)
                .ok_or_else(|| anyhow::anyhow!("provider {provider_id} not found"))?;
            let provider = providers::build_provider_from_config(&shared::ProviderConfig {
                name: record.name.clone(),
                provider_type: record.provider_type.clone(),
                base_url: record.base_url.clone(),
                api_key: record.api_key_secret_ref.clone(),
                model: record.model.clone(),
                supports_streaming: true,
                supports_json_mode: true,
                max_context_tokens: None,
                request_timeout_seconds: 120,
                stream_idle_timeout_seconds: 30,
                max_retries: 1,
                http_referer: None,
                x_title: None,
                provider_routing: None,
                include_usage: true,
            })?;
            let models = provider.list_models().await?;
            print_json(&models)
        }
    }
}
