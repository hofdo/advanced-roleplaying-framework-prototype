use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod bootstrap;
mod commands;
mod render;
mod samples;
mod scenario_io;

use bootstrap::{CliRuntimeOptions, build_state};

#[derive(Parser, Debug)]
#[command(
    name = "rp",
    version,
    about = "Roleplay engine CLI — dogfood the engine from the terminal"
)]
struct Cli {
    /// Use Postgres storage instead of the in-memory store.
    #[arg(long, global = true, env = "ROLEPLAY_CLI_POSTGRES")]
    postgres: bool,

    /// Optional path to an AppConfig TOML file.
    #[arg(long, global = true)]
    config: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Manage scenarios.
    #[command(subcommand)]
    Scenario(commands::scenario::Cmd),
    /// Manage sessions.
    #[command(subcommand)]
    Session(commands::session::Cmd),
    /// Submit a turn against a session.
    Turn(commands::turn::Args),
    /// Show the projected world state for a session.
    World(commands::world::Args),
    /// Manage persisted provider configurations (Postgres only).
    #[command(subcommand)]
    Provider(commands::provider::Cmd),
    /// Start a local development stack and enter chat mode.
    #[command(subcommand)]
    Dev(commands::dev::Cmd),
    /// Interactive chat REPL — type turns and slash-commands in one session.
    Chat(commands::chat::Args),
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("RP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(cli: Cli) -> Result<()> {
    let config_path = cli.config;
    let runtime = CliRuntimeOptions {
        use_postgres: cli.postgres,
        config_path: config_path.clone(),
    };

    match cli.command {
        Command::Scenario(cmd) => {
            let state = build_state(runtime).await?;
            commands::scenario::run(state, cmd).await
        }
        Command::Session(cmd) => {
            let state = build_state(runtime).await?;
            commands::session::run(state, cmd).await
        }
        Command::Turn(args) => {
            let state = build_state(runtime).await?;
            commands::turn::run(state, args).await
        }
        Command::World(args) => {
            let state = build_state(runtime).await?;
            commands::world::run(state, args).await
        }
        Command::Provider(cmd) => {
            let state = build_state(runtime).await?;
            commands::provider::run(state, cmd).await
        }
        Command::Dev(cmd) => commands::dev::run(config_path.as_deref(), cmd).await,
        Command::Chat(args) => {
            let state = build_state(runtime).await?;
            commands::chat::run(state, args).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn chat_parses_view_flag() {
        let cli = Cli::try_parse_from(["rp", "chat", "--sample", "chosen-beyond-goddess", "--view", "quiet"])
            .expect("chat args should parse");

        match cli.command {
            Command::Chat(args) => {
                assert_eq!(args.view, crate::render::OutputView::Quiet);
            }
            other => panic!("expected chat command, got {other:?}"),
        }
    }

    #[test]
    fn turn_parses_view_flag() {
        let cli = Cli::try_parse_from([
            "rp",
            "turn",
            "00000000-0000-0000-0000-000000000000",
            "--input",
            "hello",
            "--view",
            "verbose",
        ])
        .expect("turn args should parse");

        match cli.command {
            Command::Turn(args) => {
                assert_eq!(args.view, crate::render::OutputView::Verbose);
            }
            other => panic!("expected turn command, got {other:?}"),
        }
    }

    #[test]
    fn dev_local_parses_view_flag() {
        let cli = Cli::try_parse_from(["rp", "dev", "local", "--view", "quiet"])
            .expect("dev local args should parse");

        match cli.command {
            Command::Dev(commands::dev::Cmd::Local(args)) => {
                assert_eq!(args.view, crate::render::OutputView::Quiet);
            }
            other => panic!("expected dev local command, got {other:?}"),
        }
    }

    #[test]
    fn dev_openrouter_parses_view_flag() {
        let cli = Cli::try_parse_from(["rp", "dev", "open-router", "--view", "verbose"])
            .expect("dev openrouter args should parse");

        match cli.command {
            Command::Dev(commands::dev::Cmd::OpenRouter(args)) => {
                assert_eq!(args.view, crate::render::OutputView::Verbose);
            }
            other => panic!("expected dev openrouter command, got {other:?}"),
        }
    }
}
