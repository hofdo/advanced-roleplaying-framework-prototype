use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use reqwest::Client;
use shared::{AppConfig, StorageBackend};
use tokio::time::sleep;
use uuid::Uuid;

use crate::{bootstrap::build_state_from_config, commands::chat};

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Start Postgres plus the local llama.cpp stack, then open chat mode.
    Local(LocalArgs),
    /// Start Postgres plus OpenRouter, then open chat mode.
    OpenRouter(OpenRouterArgs),
}

#[derive(Args, Debug, Clone)]
pub struct LocalArgs {
    /// Built-in sample to load into the chat session when no scenario is selected.
    #[arg(long, conflicts_with = "scenario")]
    pub sample: Option<String>,
    /// Existing scenario to load into the chat session.
    #[arg(long, conflicts_with = "sample")]
    pub scenario: Option<Uuid>,
    /// Hugging Face model alias used by scripts/start-llm.sh.
    #[arg(long, default_value = "gemma4-uncensored")]
    pub model: String,
    /// Optional llama-server port override. Defaults to LLAMA_PORT or 8080.
    #[arg(long, env = "LLAMA_PORT")]
    pub llama_port: Option<u16>,
    /// Remove the owned Postgres stack on exit instead of stopping it.
    #[arg(long)]
    pub destroy: bool,
    /// Terminal rendering style for chat output.
    #[arg(long, value_enum, default_value_t = crate::render::OutputView::Verbose)]
    pub view: crate::render::OutputView,
}

#[derive(Args, Debug, Clone)]
pub struct OpenRouterArgs {
    /// Built-in sample to load into the chat session when no scenario is selected.
    #[arg(long, conflicts_with = "scenario")]
    pub sample: Option<String>,
    /// Existing scenario to load into the chat session.
    #[arg(long, conflicts_with = "sample")]
    pub scenario: Option<Uuid>,
    /// Remove the owned Postgres stack on exit instead of stopping it.
    #[arg(long)]
    pub destroy: bool,
    /// Terminal rendering style for chat output.
    #[arg(long, value_enum, default_value_t = crate::render::OutputView::Verbose)]
    pub view: crate::render::OutputView,
}

pub async fn run(config_path: Option<&str>, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Local(args) => run_local(config_path, args).await,
        Cmd::OpenRouter(args) => run_openrouter(config_path, args).await,
    }
}

async fn run_local(config_path: Option<&str>, args: LocalArgs) -> Result<()> {
    let mut config = load_config(config_path)?;
    let port = args.llama_port.unwrap_or(8080);

    configure_local_runtime(&mut config, port);
    let _postgres = ensure_postgres_service(args.destroy)?;
    let mut llama = start_local_llm(&args.model, port)?;
    wait_for_http_ready(
        &mut llama,
        &format!("http://127.0.0.1:{port}/health"),
        "local llama health",
    )
    .await?;
    wait_for_http_ready(
        &mut llama,
        &format!("http://127.0.0.1:{port}/props"),
        "local llama props",
    )
    .await?;

    let state = build_state_from_config(config).await?;
    let chat_args = build_chat_args(args.sample, args.scenario, args.view);
    chat::run(state, chat_args).await
}

async fn run_openrouter(config_path: Option<&str>, args: OpenRouterArgs) -> Result<()> {
    let mut config = load_config(config_path)?;
    configure_openrouter_runtime(&mut config);
    let _postgres = ensure_postgres_service(args.destroy)?;

    let state = build_state_from_config(config).await?;
    let chat_args = build_chat_args(args.sample, args.scenario, args.view);
    chat::run(state, chat_args).await
}

fn load_config(config_path: Option<&str>) -> Result<AppConfig> {
    let path = config_path.map(PathBuf::from);
    let mut config = AppConfig::load(path.as_deref())?;
    config.storage.backend = StorageBackend::Postgres;
    config.storage.migrate_on_startup = true;
    Ok(config)
}

fn configure_local_runtime(config: &mut AppConfig, port: u16) {
    config.provider.default.name = "local-llama".into();
    config.provider.default.provider_type = "llama_cpp".into();
    config.provider.default.base_url = format!("http://127.0.0.1:{port}/v1");
    config.provider.default.model = "local-model".into();
    config.provider.default.api_key = None;
    config.provider.default.supports_streaming = true;
    config.provider.default.supports_json_mode = false;
    config.storage.backend = StorageBackend::Postgres;
    config.storage.migrate_on_startup = true;
}

fn configure_openrouter_runtime(config: &mut AppConfig) {
    config.provider.default.name = "openrouter".into();
    config.provider.default.provider_type = "openrouter".into();
    config.provider.default.base_url = "https://openrouter.ai/api/v1".into();
    config.provider.default.model = "openai/gpt-4o-mini".into();
    config.provider.default.api_key = Some("env:OPENROUTER_API_KEY".into());
    config.provider.default.supports_streaming = true;
    config.provider.default.supports_json_mode = false;
    config.storage.backend = StorageBackend::Postgres;
    config.storage.migrate_on_startup = true;
}

fn build_chat_args(
    sample: Option<String>,
    scenario: Option<Uuid>,
    view: crate::render::OutputView,
) -> chat::Args {
    let sample = match scenario {
        Some(_) => None,
        None => Some(sample.unwrap_or_else(|| "chosen-beyond-goddess".into())),
    };

    chat::Args {
        session: None,
        scenario,
        sample,
        mode: None,
        admin: false,
        view,
    }
}

fn ensure_postgres_service(destroy: bool) -> Result<ManagedComposeService> {
    let root = repo_root();
    if compose_service_running(&root, "postgres")? {
        wait_for_postgres_healthy(&root)?;
        return Ok(ManagedComposeService {
            root,
            service: "postgres".into(),
            owned: false,
            destroy,
        });
    }

    let status = Command::new("docker")
        .args(["compose", "up", "-d", "postgres"])
        .current_dir(&root)
        .status()
        .context("starting postgres with docker compose")?;
    if !status.success() {
        bail!("docker compose up -d postgres failed with status {status}");
    }

    wait_for_postgres_healthy(&root)?;
    Ok(ManagedComposeService {
        root,
        service: "postgres".into(),
        owned: true,
        destroy,
    })
}

fn start_local_llm(model: &str, port: u16) -> Result<ManagedProcess> {
    let script = repo_root().join("scripts/start-llm.sh");
    let child = Command::new("bash")
        .arg(script)
        .arg(model)
        .env("LLAMA_PORT", port.to_string())
        .current_dir(repo_root())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("starting local llama-server")?;
    Ok(ManagedProcess { child })
}

async fn wait_for_http_ready(child: &mut ManagedProcess, url: &str, label: &str) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .context("building HTTP readiness client")?;

    for _ in 0..180 {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("polling {label} process"))?
        {
            bail!("{label} exited early with status {status}");
        }

        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(_) | Err(_) => {
                sleep(Duration::from_secs(2)).await;
            }
        }
    }

    bail!("{label} did not become ready at {url}");
}

fn wait_for_postgres_healthy(root: &Path) -> Result<()> {
    for _ in 0..60 {
        let output = Command::new("docker")
            .args(["compose", "ps", "postgres"])
            .current_dir(root)
            .output()
            .context("checking postgres health")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("healthy") {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    bail!("postgres did not become healthy");
}

fn compose_service_running(root: &Path, service: &str) -> Result<bool> {
    let output = Command::new("docker")
        .args(["compose", "ps", "-q", service])
        .current_dir(root)
        .output()
        .with_context(|| format!("checking compose service {service}"))?;
    if !output.status.success() {
        bail!(
            "docker compose ps -q {service} failed with status {}",
            output.status
        );
    }

    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if container_id.is_empty() {
        return Ok(false);
    }

    let inspect = Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", &container_id])
        .output()
        .with_context(|| format!("inspecting compose service {service}"))?;
    if !inspect.status.success() {
        return Ok(false);
    }

    Ok(String::from_utf8_lossy(&inspect.stdout).trim() == "true")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("cli crate should live under crates/cli")
        .to_path_buf()
}

struct ManagedProcess {
    child: Child,
}

impl ManagedProcess {
    fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child
            .try_wait()
            .context("polling local llama-server process")
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct ManagedComposeService {
    root: PathBuf,
    service: String,
    owned: bool,
    destroy: bool,
}

impl Drop for ManagedComposeService {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }

        let command: Vec<&str> = if self.destroy {
            vec!["compose", "down"]
        } else {
            vec!["compose", "stop", self.service.as_str()]
        };

        let _ = Command::new("docker")
            .args(command)
            .current_dir(&self.root)
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::{build_chat_args, configure_local_runtime, configure_openrouter_runtime};
    use shared::{AppConfig, StorageBackend};
    use uuid::Uuid;

    #[test]
    fn local_runtime_overrides_provider_defaults() {
        let mut config = AppConfig::default();
        configure_local_runtime(&mut config, 8080);

        assert_eq!(config.storage.backend, StorageBackend::Postgres);
        assert_eq!(config.provider.default.name, "local-llama");
        assert_eq!(config.provider.default.provider_type, "llama_cpp");
        assert_eq!(config.provider.default.base_url, "http://127.0.0.1:8080/v1");
        assert_eq!(config.provider.default.model, "local-model");
    }

    #[test]
    fn openrouter_runtime_overrides_provider_defaults() {
        let mut config = AppConfig::default();
        configure_openrouter_runtime(&mut config);

        assert_eq!(config.storage.backend, StorageBackend::Postgres);
        assert_eq!(config.provider.default.name, "openrouter");
        assert_eq!(config.provider.default.provider_type, "openrouter");
        assert_eq!(
            config.provider.default.base_url,
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(config.provider.default.model, "openai/gpt-4o-mini");
        assert_eq!(
            config.provider.default.api_key.as_deref(),
            Some("env:OPENROUTER_API_KEY")
        );
    }

    #[test]
    fn build_chat_args_defaults_to_sample_when_unset() {
        let args = build_chat_args(None, None, crate::render::OutputView::Quiet);

        assert_eq!(args.sample.as_deref(), Some("chosen-beyond-goddess"));
        assert_eq!(args.scenario, None);
        assert_eq!(args.session, None);
        assert_eq!(args.view, crate::render::OutputView::Quiet);
    }

    #[test]
    fn build_chat_args_prefers_scenario_over_sample() {
        let scenario = Uuid::new_v4();
        let args = build_chat_args(
            Some("chosen-beyond-goddess".into()),
            Some(scenario),
            crate::render::OutputView::Verbose,
        );

        assert_eq!(args.sample, None);
        assert_eq!(args.scenario, Some(scenario));
        assert_eq!(args.session, None);
        assert_eq!(args.view, crate::render::OutputView::Verbose);
    }
}
