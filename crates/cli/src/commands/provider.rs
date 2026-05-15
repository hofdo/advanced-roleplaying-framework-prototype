use anyhow::{Context, Result};
use clap::Subcommand;
use persistence::ProviderRecord;
use providers::LlmProvider;
use shared::StorageBackend;
use std::sync::Arc;
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
    /// Show configured provider readiness and registered provider metadata.
    Status,
    /// Run health and readiness checks for a configured or registered provider.
    Test {
        #[arg(long)]
        provider_id: Option<Uuid>,
    },
}

pub async fn run(state: CliState, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Status => print_provider_status(&state).await,
        Cmd::Test { provider_id } => print_provider_test(&state, provider_id).await,
        Cmd::Register { file } => {
            require_postgres(&state)?;
            let bytes = std::fs::read(&file).with_context(|| format!("reading {file}"))?;
            let record: ProviderRecord = serde_json::from_slice(&bytes)
                .with_context(|| format!("parsing provider record {file}"))?;
            let saved = state.store.create_provider(record).await?;
            print_json(&saved)
        }
        Cmd::List => {
            require_postgres(&state)?;
            let records = state.store.list_providers().await?;
            print_json(&records)
        }
        Cmd::Remove { provider_id } => {
            require_postgres(&state)?;
            state.store.delete_provider(provider_id).await?;
            println!("removed provider {provider_id}");
            Ok(())
        }
        Cmd::Models { provider_id } => {
            require_postgres(&state)?;
            let record = state
                .provider_record(provider_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("provider {provider_id} not found"))?;
            let provider = state.build_provider_from_record(&record)?;
            let models = provider.list_models().await?;
            print_json(&models)
        }
    }
}

fn require_postgres(state: &CliState) -> Result<()> {
    if matches!(state.config.storage.backend, StorageBackend::Memory) {
        anyhow::bail!("provider management requires --postgres");
    }
    Ok(())
}

async fn print_provider_status(state: &CliState) -> Result<()> {
    let health = state.provider.health().await?;
    let readiness = state.provider.readiness().await?;

    println!("storage: {}", state.store.storage_status().await);
    println!("default: {}", state.config.provider.default.name);
    println!("health: ok={}", health.ok);
    if let Some(message) = health.message {
        println!("health message: {message}");
    }
    println!(
        "readiness: configured={} reachable={}",
        readiness.configured, readiness.reachable
    );
    println!("readiness message: {}", readiness.message);

    match state.config.storage.backend {
        StorageBackend::Memory => {
            println!("registered providers: unavailable in memory mode");
        }
        StorageBackend::Postgres => {
            let records = state.store.list_providers().await?;
            if records.is_empty() {
                println!("registered providers: none");
            } else {
                println!("registered providers:");
                for record in records {
                    println!(
                        "- {} {} type={} model={} default={}",
                        record.id,
                        record.name,
                        record.provider_type,
                        record.model,
                        record.is_default
                    );
                }
            }
        }
    }

    Ok(())
}

async fn print_provider_test(state: &CliState, provider_id: Option<Uuid>) -> Result<()> {
    let provider = match provider_id {
        Some(provider_id) => {
            if matches!(state.config.storage.backend, StorageBackend::Memory) {
                anyhow::bail!(
                    "provider lookup by id is unavailable in memory mode; test the configured default provider without --provider-id"
                );
            }
            state.resolve_provider_by_id(provider_id).await?
        }
        None => Arc::clone(&state.provider),
    };

    print_provider_diagnostics(provider.as_ref()).await
}

async fn print_provider_diagnostics(provider: &dyn LlmProvider) -> Result<()> {
    let health = provider.health().await?;
    let readiness = provider.readiness().await?;

    println!("name: {}", health.name);
    println!("health: ok={}", health.ok);
    if let Some(message) = health.message {
        println!("health message: {message}");
    }
    println!(
        "readiness: configured={} reachable={}",
        readiness.configured, readiness.reachable
    );
    println!("readiness message: {}", readiness.message);
    Ok(())
}
