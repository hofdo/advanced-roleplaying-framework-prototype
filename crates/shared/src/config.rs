use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{fs, net::Ipv4Addr, path::Path, str::FromStr};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub storage: StorageConfig,
    pub provider: ProviderSection,
    pub debug: DebugConfig,
}

impl AppConfig {
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        let mut config = if let Some(path) = path {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read config file {}", path.display()))?;
            toml::from_str(&text)
                .with_context(|| format!("failed to parse config file {}", path.display()))?
        } else {
            Self::default()
        };

        if let Ok(url) = std::env::var("DATABASE_URL") {
            config.database.url = url;
        }
        if let Ok(storage) = std::env::var("ROLEPLAY_STORAGE") {
            config.storage.backend = storage.parse()?;
        }
        if let Ok(port) = std::env::var("PORT") {
            config.server.port = port.parse().context("PORT must be a u16")?;
        }
        if let Ok(base_url) = std::env::var("LLM_BASE_URL") {
            config.provider.default.base_url = base_url;
        }
        if let Ok(model) = std::env::var("LLM_MODEL") {
            config.provider.default.model = model;
        }
        if let Ok(api_key) = std::env::var("LLM_API_KEY") {
            config.provider.default.api_key = Some(api_key);
        }

        Ok(config)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: Ipv4Addr::new(0, 0, 0, 0),
                port: 8080,
            },
            database: DatabaseConfig {
                url: "postgres://roleplay:roleplay@localhost:5432/roleplay".into(),
            },
            storage: StorageConfig {
                backend: StorageBackend::Postgres,
                migrate_on_startup: true,
            },
            provider: ProviderSection {
                default: ProviderConfig {
                    name: "local-llama".into(),
                    provider_type: "openai_compatible".into(),
                    base_url: "http://localhost:8081/v1".into(),
                    api_key: None,
                    model: "local-model".into(),
                    supports_streaming: true,
                    supports_json_mode: false,
                    max_context_tokens: Some(32_768),
                    request_timeout_seconds: 120,
                    stream_idle_timeout_seconds: 30,
                    max_retries: 1,
                },
            },
            debug: DebugConfig {
                store_raw_provider_output: false,
                allow_debug_state: false,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerConfig {
    pub host: Ipv4Addr,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageConfig {
    pub backend: StorageBackend,
    pub migrate_on_startup: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackend {
    Postgres,
    Memory,
}

impl FromStr for StorageBackend {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "postgres" | "postgresql" | "pg" => Ok(Self::Postgres),
            "memory" | "in_memory" | "in-memory" => Ok(Self::Memory),
            other => anyhow::bail!(
                "unsupported ROLEPLAY_STORAGE value '{other}', expected 'postgres' or 'memory'"
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderSection {
    pub default: ProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConfig {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub supports_streaming: bool,
    pub supports_json_mode: bool,
    pub max_context_tokens: Option<u32>,
    pub request_timeout_seconds: u64,
    pub stream_idle_timeout_seconds: u64,
    pub max_retries: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DebugConfig {
    pub store_raw_provider_output: bool,
    pub allow_debug_state: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_local_openai_compatible_provider() {
        let config = AppConfig::default();

        assert_eq!(config.server.port, 8080);
        assert_eq!(config.storage.backend, StorageBackend::Postgres);
        assert!(config.storage.migrate_on_startup);
        assert_eq!(config.provider.default.base_url, "http://localhost:8081/v1");
        assert!(!config.debug.store_raw_provider_output);
    }

    #[test]
    fn storage_backend_parses_memory_override() {
        assert_eq!(
            "memory".parse::<StorageBackend>().unwrap(),
            StorageBackend::Memory
        );
        assert_eq!(
            "postgres".parse::<StorageBackend>().unwrap(),
            StorageBackend::Postgres
        );
    }
}
