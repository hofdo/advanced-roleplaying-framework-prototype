//! Interactive `rp chat` mode.
//!
//! The REPL holds active scenario / session / mode / admin / stream state
//! in-process so multiple turns and slash-commands can share one engine
//! lifetime. Plain text lines become turns; `/`-prefixed lines are slash
//! commands that mirror the one-shot subcommands.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use domain::{ScenarioId, SessionId, TurnMode, ViewerContext};
use engine::{DefaultTurnPipeline, FrontendStateProjector, TurnRequestInput};
use rustyline::{Editor, error::ReadlineError, history::FileHistory};
use uuid::Uuid;

use crate::{
    bootstrap::CliState,
    render::{print_json, render_streaming_turn},
    samples::{build_sample, sample_names},
    scenario_io::read_scenario_file,
};

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Resume an existing session by id.
    #[arg(long)]
    pub session: Option<Uuid>,
    /// Use an existing scenario; a new session is created for it.
    #[arg(long, conflicts_with = "session")]
    pub scenario: Option<Uuid>,
    /// Create a built-in sample scenario and a fresh session for it.
    #[arg(long, conflicts_with_all = ["session", "scenario"])]
    pub sample: Option<String>,
    /// Default turn mode for plain-text turns. Omit to let the scene
    /// classifier decide (the `auto` setting).
    #[arg(long, value_enum)]
    pub mode: Option<super::turn::Mode>,
    /// Start in admin viewer mode (sees GM-only facts).
    #[arg(long)]
    pub admin: bool,
}

struct ChatState {
    cli: CliState,
    active_scenario: Option<ScenarioId>,
    active_session: Option<SessionId>,
    mode: Option<TurnMode>,
    stream: bool,
    admin: bool,
}

impl ChatState {
    fn viewer(&self) -> ViewerContext {
        if self.admin {
            ViewerContext {
                include_debug_state: true,
                is_admin: true,
            }
        } else {
            ViewerContext::player()
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum SlashCmd {
    Help,
    Exit,
    Status,
    ScenarioCreate {
        sample: Option<String>,
        file: Option<String>,
    },
    ScenarioList,
    ScenarioUse(Uuid),
    SessionNew {
        title: Option<String>,
    },
    SessionList,
    SessionUse(Uuid),
    SessionShow,
    World {
        admin: bool,
    },
    Mode(Option<TurnMode>),
    Stream(bool),
    Admin(bool),
    Unknown(String),
    Empty,
}

pub(crate) fn parse_slash(line: &str) -> Option<SlashCmd> {
    let trimmed = line.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let body = &trimmed[1..];
    if body.is_empty() {
        return Some(SlashCmd::Empty);
    }
    let mut tokens = body.split_whitespace();
    let head = tokens.next().unwrap_or("");
    let rest: Vec<&str> = tokens.collect();
    Some(match head {
        "help" | "h" | "?" => SlashCmd::Help,
        "exit" | "quit" | "q" => SlashCmd::Exit,
        "status" => SlashCmd::Status,
        "scenario" => parse_scenario(&rest),
        "session" => parse_session(&rest),
        "world" => SlashCmd::World {
            admin: rest.iter().any(|t| *t == "--admin"),
        },
        "mode" => match rest.first().copied() {
            Some("auto") | None => SlashCmd::Mode(None),
            Some("action") => SlashCmd::Mode(Some(TurnMode::Action)),
            Some("dialogue") => SlashCmd::Mode(Some(TurnMode::Dialogue)),
            Some("direct") => SlashCmd::Mode(Some(TurnMode::Direct)),
            Some("remember") => SlashCmd::Mode(Some(TurnMode::Remember)),
            Some(other) => SlashCmd::Unknown(format!("unknown mode: {other}")),
        },
        "stream" => match rest.first().copied() {
            Some("on") => SlashCmd::Stream(true),
            Some("off") => SlashCmd::Stream(false),
            _ => SlashCmd::Unknown("usage: /stream on|off".into()),
        },
        "admin" => match rest.first().copied() {
            Some("on") => SlashCmd::Admin(true),
            Some("off") => SlashCmd::Admin(false),
            _ => SlashCmd::Unknown("usage: /admin on|off".into()),
        },
        other => SlashCmd::Unknown(format!("unknown slash command: /{other}. Try /help.")),
    })
}

fn parse_scenario(rest: &[&str]) -> SlashCmd {
    match rest.first().copied() {
        Some("create") => {
            let mut sample = None;
            let mut file = None;
            let mut i = 1;
            while i < rest.len() {
                match rest[i] {
                    "--sample" => {
                        sample = rest.get(i + 1).map(|s| s.to_string());
                        i += 2;
                    }
                    "--file" => {
                        file = rest.get(i + 1).map(|s| s.to_string());
                        i += 2;
                    }
                    other => {
                        return SlashCmd::Unknown(format!("unknown flag: {other}"));
                    }
                }
            }
            SlashCmd::ScenarioCreate { sample, file }
        }
        Some("list") => SlashCmd::ScenarioList,
        Some("use") => match rest.get(1).and_then(|id| Uuid::parse_str(id).ok()) {
            Some(id) => SlashCmd::ScenarioUse(id),
            None => SlashCmd::Unknown("usage: /scenario use <UUID>".into()),
        },
        _ => SlashCmd::Unknown(
            "usage: /scenario create [--sample NAME | --file PATH] | /scenario list | /scenario use <UUID>"
                .into(),
        ),
    }
}

fn parse_session(rest: &[&str]) -> SlashCmd {
    match rest.first().copied() {
        Some("new") => {
            // `--title` consumes the remainder of the line so users can supply
            // multi-word titles without quoting: `/session new --title my run`.
            let title = if let Some(idx) = rest.iter().position(|t| *t == "--title") {
                let remainder = rest[idx + 1..].join(" ");
                if remainder.is_empty() {
                    None
                } else {
                    Some(remainder)
                }
            } else {
                None
            };
            SlashCmd::SessionNew { title }
        }
        Some("list") => SlashCmd::SessionList,
        Some("show") => SlashCmd::SessionShow,
        Some("use") => match rest.get(1).and_then(|id| Uuid::parse_str(id).ok()) {
            Some(id) => SlashCmd::SessionUse(id),
            None => SlashCmd::Unknown("usage: /session use <UUID>".into()),
        },
        _ => SlashCmd::Unknown(
            "usage: /session new [--title TEXT] | /session list | /session use <UUID> | /session show"
                .into(),
        ),
    }
}

const HELP_TEXT: &str = "\
chat commands (prefix with /):
  /help                            Show this help
  /exit, /quit                     Leave the REPL
  /status                          Print active scenario, session, mode, etc.
  /scenario create [--sample NAME | --file PATH]
  /scenario list                   List scenarios
  /scenario use <UUID>             Select active scenario
  /session new [--title TEXT]      New session for the active scenario
  /session list                    List sessions
  /session use <UUID>              Select active session
  /session show                    Print active session
  /world [--admin]                 Show projected (or raw) world state
  /mode <action|dialogue|direct|remember|auto>
  /stream <on|off>                 Toggle streaming output (default on)
  /admin <on|off>                  Toggle admin viewer
plain text submits a turn against the active session.";

/// Source of input lines. Production uses a rustyline wrapper; tests use a
/// `Vec<String>` iterator. Returning `Ok(None)` terminates the loop the same
/// way `Ctrl+D` does in the interactive editor.
pub(crate) trait LineSource {
    fn read_line(&mut self, prompt: &str) -> std::io::Result<Option<String>>;
    fn record_history(&mut self, _line: &str) {}
}

struct RustylineSource {
    editor: Editor<(), FileHistory>,
    history_path: Option<PathBuf>,
}

impl RustylineSource {
    fn new(history_path: Option<PathBuf>) -> Result<Self> {
        let mut editor: Editor<(), FileHistory> =
            Editor::new().context("failed to initialize line editor")?;
        if let Some(path) = history_path.as_ref() {
            let _ = editor.load_history(path);
        }
        Ok(Self {
            editor,
            history_path,
        })
    }
}

impl LineSource for RustylineSource {
    fn read_line(&mut self, prompt: &str) -> std::io::Result<Option<String>> {
        match self.editor.readline(prompt) {
            Ok(line) => Ok(Some(line)),
            Err(ReadlineError::Interrupted) => {
                println!("use /exit to quit (or Ctrl+D).");
                Ok(Some(String::new()))
            }
            Err(ReadlineError::Eof) => Ok(None),
            Err(error) => Err(std::io::Error::new(std::io::ErrorKind::Other, error)),
        }
    }

    fn record_history(&mut self, line: &str) {
        self.editor.add_history_entry(line).ok();
    }
}

impl Drop for RustylineSource {
    fn drop(&mut self) {
        if let Some(path) = self.history_path.as_ref() {
            let _ = self.editor.save_history(path);
        }
    }
}

/// Scripted line source for tests. Each call to `read_line` pops the front of
/// the queue and returns it; an exhausted queue terminates the loop.
#[cfg(test)]
pub(crate) struct ScriptedLines {
    lines: std::collections::VecDeque<String>,
}

#[cfg(test)]
impl ScriptedLines {
    pub(crate) fn new<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self {
            lines: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
impl LineSource for ScriptedLines {
    fn read_line(&mut self, _prompt: &str) -> std::io::Result<Option<String>> {
        Ok(self.lines.pop_front())
    }
}

pub async fn run(state: CliState, args: Args) -> Result<()> {
    let history_path = history_file_path()?;
    let mut source = RustylineSource::new(history_path)?;
    run_with_source(state, args, &mut source).await
}

/// Same as [`run`] but with a pluggable [`LineSource`] for testability.
pub(crate) async fn run_with_source<L: LineSource>(
    state: CliState,
    args: Args,
    source: &mut L,
) -> Result<()> {
    let mode: Option<TurnMode> = args.mode.map(Into::into);
    let mut chat = ChatState {
        cli: state,
        active_scenario: None,
        active_session: None,
        mode,
        stream: true,
        admin: args.admin,
    };

    // Initial state from startup flags.
    if let Some(name) = args.sample.as_deref() {
        let scenario = build_sample(name)?;
        let scenario = chat.cli.store.create_scenario(scenario).await?;
        chat.active_scenario = Some(scenario.id);
        let session = chat
            .cli
            .store
            .create_session(scenario.id, "CLI Chat".into())
            .await?
            .ok_or_else(|| anyhow::anyhow!("could not create session for sample scenario"))?;
        chat.active_session = Some(session.id);
        println!(
            "loaded sample scenario {} (session {})",
            scenario.id, session.id
        );
    } else if let Some(scenario_id) = args.scenario {
        let scenario = chat
            .cli
            .store
            .get_scenario(scenario_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("scenario {scenario_id} not found"))?;
        chat.active_scenario = Some(scenario.id);
        let session = chat
            .cli
            .store
            .create_session(scenario.id, "CLI Chat".into())
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("could not create session for scenario {scenario_id}")
            })?;
        chat.active_session = Some(session.id);
        println!("active scenario {} (session {})", scenario.id, session.id);
    } else if let Some(session_id) = args.session {
        let session = chat
            .cli
            .store
            .get_session(session_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("session {session_id} not found"))?;
        chat.active_scenario = Some(session.scenario_id);
        chat.active_session = Some(session.id);
        println!(
            "resumed session {} (scenario {})",
            session.id, session.scenario_id
        );
    } else {
        println!(
            "no session selected. Use /scenario create --sample <{}>, or start with --session/--scenario/--sample.",
            sample_names().join("|")
        );
    }

    println!("type /help for commands, /exit to quit.");

    loop {
        let prompt = build_prompt(&chat);
        let line = match source.read_line(&prompt) {
            Ok(Some(line)) => line,
            Ok(None) => {
                println!("goodbye.");
                break;
            }
            Err(error) => {
                eprintln!("input error: {error}");
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        source.record_history(trimmed);

        if let Some(slash) = parse_slash(trimmed) {
            match handle_slash(&mut chat, slash).await {
                ControlFlow::Continue => {}
                ControlFlow::Exit => break,
                ControlFlow::Error(err) => eprintln!("error: {err:#}"),
            }
            continue;
        }

        match handle_turn(&mut chat, trimmed.to_owned()).await {
            Ok(()) => {}
            Err(err) => eprintln!("error: {err:#}"),
        }
    }

    Ok(())
}

enum ControlFlow {
    Continue,
    Exit,
    Error(anyhow::Error),
}

async fn handle_slash(chat: &mut ChatState, cmd: SlashCmd) -> ControlFlow {
    match cmd {
        SlashCmd::Help => {
            println!("{HELP_TEXT}");
        }
        SlashCmd::Exit => {
            println!("goodbye.");
            return ControlFlow::Exit;
        }
        SlashCmd::Empty => {}
        SlashCmd::Status => print_status(chat),
        SlashCmd::ScenarioCreate { sample, file } => {
            match cmd_scenario_create(chat, sample, file).await {
                Ok(()) => {}
                Err(e) => return ControlFlow::Error(e),
            }
        }
        SlashCmd::ScenarioList => match chat.cli.store.list_scenarios().await {
            Ok(scenarios) => {
                if scenarios.is_empty() {
                    println!("no scenarios.");
                } else {
                    for s in scenarios {
                        println!("{}  {}", s.id, s.title);
                    }
                }
            }
            Err(e) => return ControlFlow::Error(e.into()),
        },
        SlashCmd::ScenarioUse(id) => match chat.cli.store.get_scenario(id).await {
            Ok(Some(_)) => {
                chat.active_scenario = Some(id);
                chat.active_session = None;
                println!("active scenario {id} (no session — use /session new)");
            }
            Ok(None) => println!("scenario {id} not found."),
            Err(e) => return ControlFlow::Error(e.into()),
        },
        SlashCmd::SessionNew { title } => {
            let Some(scenario_id) = chat.active_scenario else {
                println!("no active scenario. Use /scenario create or /scenario use first.");
                return ControlFlow::Continue;
            };
            let title = title.unwrap_or_else(|| "CLI Chat".into());
            match chat.cli.store.create_session(scenario_id, title).await {
                Ok(Some(session)) => {
                    chat.active_session = Some(session.id);
                    println!("active session {}", session.id);
                }
                Ok(None) => println!("scenario {scenario_id} not found."),
                Err(e) => return ControlFlow::Error(e.into()),
            }
        }
        SlashCmd::SessionList => match chat.cli.store.list_sessions().await {
            Ok(sessions) => {
                if sessions.is_empty() {
                    println!("no sessions.");
                } else {
                    for s in sessions {
                        println!("{}  scenario={}  title={}", s.id, s.scenario_id, s.title);
                    }
                }
            }
            Err(e) => return ControlFlow::Error(e.into()),
        },
        SlashCmd::SessionUse(id) => match chat.cli.store.get_session(id).await {
            Ok(Some(session)) => {
                chat.active_session = Some(session.id);
                chat.active_scenario = Some(session.scenario_id);
                println!(
                    "active session {} (scenario {})",
                    session.id, session.scenario_id
                );
            }
            Ok(None) => println!("session {id} not found."),
            Err(e) => return ControlFlow::Error(e.into()),
        },
        SlashCmd::SessionShow => {
            let Some(id) = chat.active_session else {
                println!("no active session.");
                return ControlFlow::Continue;
            };
            match chat.cli.store.get_session(id).await {
                Ok(Some(session)) => {
                    if let Err(e) = print_json(&session) {
                        return ControlFlow::Error(e);
                    }
                }
                Ok(None) => println!("session {id} not found."),
                Err(e) => return ControlFlow::Error(e.into()),
            }
        }
        SlashCmd::World { admin } => {
            if let Err(e) = cmd_world(chat, admin).await {
                return ControlFlow::Error(e);
            }
        }
        SlashCmd::Mode(mode) => {
            chat.mode = mode;
            println!(
                "mode set to {}",
                match mode {
                    None => "auto",
                    Some(TurnMode::Action) => "action",
                    Some(TurnMode::Dialogue) => "dialogue",
                    Some(TurnMode::Direct) => "direct",
                    Some(TurnMode::Remember) => "remember",
                }
            );
        }
        SlashCmd::Stream(on) => {
            chat.stream = on;
            println!("stream {}", if on { "on" } else { "off" });
        }
        SlashCmd::Admin(on) => {
            chat.admin = on;
            println!("admin {}", if on { "on" } else { "off" });
        }
        SlashCmd::Unknown(msg) => println!("{msg}"),
    }
    ControlFlow::Continue
}

async fn cmd_scenario_create(
    chat: &mut ChatState,
    sample: Option<String>,
    file: Option<String>,
) -> Result<()> {
    let scenario = match (sample, file) {
        (Some(name), None) => build_sample(&name)?,
        (None, Some(path)) => read_scenario_file(&path)?,
        (None, None) => {
            println!(
                "usage: /scenario create --sample <{}>",
                sample_names().join("|")
            );
            return Ok(());
        }
        (Some(_), Some(_)) => {
            println!("--sample and --file are mutually exclusive.");
            return Ok(());
        }
    };
    let scenario = chat.cli.store.create_scenario(scenario).await?;
    chat.active_scenario = Some(scenario.id);
    chat.active_session = None;
    println!(
        "scenario {} created — use /session new to start a session.",
        scenario.id
    );
    Ok(())
}

async fn cmd_world(chat: &ChatState, admin: bool) -> Result<()> {
    let Some(session_id) = chat.active_session else {
        println!("no active session.");
        return Ok(());
    };
    let Some(session) = chat.cli.store.get_session(session_id).await? else {
        println!("active session {session_id} no longer exists.");
        return Ok(());
    };
    let Some(world_state) = chat.cli.store.world_state(session_id).await? else {
        println!("no world state for session {session_id}.");
        return Ok(());
    };
    if admin || chat.admin {
        return print_json(&world_state);
    }
    let Some(scenario) = chat.cli.store.get_scenario(session.scenario_id).await? else {
        println!("scenario {} not found.", session.scenario_id);
        return Ok(());
    };
    let projected = engine::BasicFrontendStateProjector.project(
        &scenario,
        &world_state,
        &ViewerContext::player(),
    );
    print_json(&projected)
}

async fn handle_turn(chat: &mut ChatState, input: String) -> Result<()> {
    let Some(session_id) = chat.active_session else {
        println!("no active session — use /scenario create then /session new, or /session use.");
        return Ok(());
    };
    let pipeline = Arc::new(DefaultTurnPipeline::with_lock(
        Arc::clone(&chat.cli.provider),
        Arc::clone(&chat.cli.store),
        chat.cli.turn_lock.clone(),
    ));
    let viewer = chat.viewer();
    let mode = chat.mode;

    if chat.stream {
        let turn_future = render_streaming_turn(pipeline, session_id, input, mode, viewer);
        tokio::pin!(turn_future);
        tokio::select! {
            result = &mut turn_future => result?,
            _ = tokio::signal::ctrl_c() => {
                println!("\n^C — turn cancelled.");
                drop(turn_future);
            }
        }
    } else {
        let response = pipeline
            .process_turn(TurnRequestInput {
                session_id,
                input,
                mode,
                viewer,
            })
            .await?;
        print_json(&serde_json::json!({
            "message_id": response.message_id,
            "player_response": response.player_response,
            "scene_type": response.scene_type,
            "world_state_version": response.world_state_version,
            "changed_entities": response.changed_entities,
            "frontend_state_patch": response.frontend_state_patch,
        }))?;
    }
    Ok(())
}

fn print_status(chat: &ChatState) {
    println!(
        "scenario: {}",
        chat.active_scenario
            .map(|id| id.to_string())
            .unwrap_or_else(|| "(none)".into())
    );
    println!(
        "session:  {}",
        chat.active_session
            .map(|id| id.to_string())
            .unwrap_or_else(|| "(none)".into())
    );
    println!(
        "mode:     {}",
        match chat.mode {
            None => "auto",
            Some(TurnMode::Action) => "action",
            Some(TurnMode::Dialogue) => "dialogue",
            Some(TurnMode::Direct) => "direct",
            Some(TurnMode::Remember) => "remember",
        }
    );
    println!("stream:   {}", if chat.stream { "on" } else { "off" });
    println!("admin:    {}", if chat.admin { "on" } else { "off" });
}

fn build_prompt(chat: &ChatState) -> String {
    let suffix = if chat.admin { "!" } else { "" };
    if chat.active_session.is_some() {
        format!("rp{suffix}> ")
    } else {
        format!("rp{suffix}? ")
    }
}

fn history_file_path() -> Result<Option<PathBuf>> {
    let Some(home) = std::env::var_os("HOME") else {
        return Ok(None);
    };
    let dir = PathBuf::from(home).join(".config").join("rp");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        // History persistence is a nicety, not a requirement.
        eprintln!("warning: could not create {}: {error}", dir.display());
        return Ok(None);
    }
    Ok(Some(dir.join("history")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_help_and_exit() {
        assert_eq!(parse_slash("/help"), Some(SlashCmd::Help));
        assert_eq!(parse_slash("/quit"), Some(SlashCmd::Exit));
        assert_eq!(parse_slash("/exit"), Some(SlashCmd::Exit));
    }

    #[test]
    fn rejects_non_slash() {
        assert!(parse_slash("hello world").is_none());
        assert!(parse_slash("  not a command").is_none());
    }

    #[test]
    fn parses_scenario_create_with_sample() {
        let cmd = parse_slash("/scenario create --sample chosen-beyond-goddess");
        assert_eq!(
            cmd,
            Some(SlashCmd::ScenarioCreate {
                sample: Some("chosen-beyond-goddess".into()),
                file: None,
            })
        );
    }

    #[test]
    fn parses_session_new_with_title() {
        let cmd = parse_slash("/session new --title smoke test");
        assert_eq!(
            cmd,
            Some(SlashCmd::SessionNew {
                title: Some("smoke test".into()),
            })
        );
    }

    #[test]
    fn parses_session_new_without_title() {
        assert_eq!(
            parse_slash("/session new"),
            Some(SlashCmd::SessionNew { title: None })
        );
    }

    #[test]
    fn parses_world_admin_flag() {
        assert_eq!(
            parse_slash("/world"),
            Some(SlashCmd::World { admin: false })
        );
        assert_eq!(
            parse_slash("/world --admin"),
            Some(SlashCmd::World { admin: true })
        );
    }

    #[test]
    fn parses_mode_variants() {
        assert_eq!(parse_slash("/mode auto"), Some(SlashCmd::Mode(None)));
        assert_eq!(
            parse_slash("/mode dialogue"),
            Some(SlashCmd::Mode(Some(TurnMode::Dialogue)))
        );
        assert!(matches!(
            parse_slash("/mode garbage"),
            Some(SlashCmd::Unknown(_))
        ));
    }

    #[test]
    fn parses_stream_admin_toggles() {
        assert_eq!(parse_slash("/stream on"), Some(SlashCmd::Stream(true)));
        assert_eq!(parse_slash("/stream off"), Some(SlashCmd::Stream(false)));
        assert_eq!(parse_slash("/admin on"), Some(SlashCmd::Admin(true)));
        assert_eq!(parse_slash("/admin off"), Some(SlashCmd::Admin(false)));
    }

    #[test]
    fn parses_uuid_arguments() {
        let id = Uuid::new_v4();
        assert_eq!(
            parse_slash(&format!("/scenario use {id}")),
            Some(SlashCmd::ScenarioUse(id))
        );
        assert_eq!(
            parse_slash(&format!("/session use {id}")),
            Some(SlashCmd::SessionUse(id))
        );
    }

    #[test]
    fn unknown_command_is_reported() {
        match parse_slash("/foobar") {
            Some(SlashCmd::Unknown(_)) => {}
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    use crate::bootstrap::CliState;
    use engine::{InMemorySessionTurnLock, SessionTurnLock};
    use persistence::{ApplicationStore, InMemoryApplicationStore};
    use providers::{LlmProvider, MockProvider};
    use shared::AppConfig;
    use std::sync::Arc;

    const DELTA_JSON: &str = r#"{
        "facts_to_add": [],
        "npc_changes": [],
        "faction_changes": [],
        "quest_changes": [],
        "clock_changes": [
            {"type":"advanced","clock_id":"fame","delta":1,"reason":"talk spreads"}
        ],
        "relationship_changes": [],
        "location_change": null,
        "event_log_entries": ["chat turn fired"]
    }"#;

    fn build_test_state(provider: Arc<MockProvider>) -> CliState {
        let store: Arc<dyn ApplicationStore> = Arc::new(InMemoryApplicationStore::new(false));
        let turn_lock: Arc<dyn SessionTurnLock> = Arc::new(InMemorySessionTurnLock::default());
        let provider_arc: Arc<dyn LlmProvider> = provider;
        CliState {
            config: AppConfig::default(),
            store,
            provider: provider_arc,
            turn_lock,
        }
    }

    #[tokio::test]
    async fn scripted_repl_runs_full_chat_cycle() {
        let provider = Arc::new(MockProvider::new(
            "mock",
            [
                // For one streaming turn: narration tokens, then delta JSON.
                "The examiner watches in silence.".into(),
                DELTA_JSON.into(),
                // For a second non-streaming turn (after /stream off): visible
                // response first, then delta extraction JSON.
                "The examiner nods.".into(),
                DELTA_JSON.into(),
            ],
        ));
        let state = build_test_state(Arc::clone(&provider));

        let mut script = ScriptedLines::new(vec![
            "/scenario create --sample chosen-beyond-goddess".to_string(),
            "/session new --title smoke".to_string(),
            "I greet the examiner.".to_string(),
            "/mode dialogue".to_string(),
            "/stream off".to_string(),
            "I greet again.".to_string(),
            "/exit".to_string(),
        ]);

        run_with_source(
            state,
            Args {
                session: None,
                scenario: None,
                sample: None,
                mode: None,
                admin: false,
            },
            &mut script,
        )
        .await
        .expect("REPL completes cleanly");
    }

    #[tokio::test]
    async fn scripted_repl_starts_with_sample_flag() {
        let provider = Arc::new(MockProvider::new(
            "mock",
            ["Tokens here".into(), DELTA_JSON.into()],
        ));
        let state = build_test_state(Arc::clone(&provider));

        let mut script = ScriptedLines::new(vec![
            "I greet the examiner.".to_string(),
            "/exit".to_string(),
        ]);

        run_with_source(
            state,
            Args {
                session: None,
                scenario: None,
                sample: Some("chosen-beyond-goddess".into()),
                mode: None,
                admin: false,
            },
            &mut script,
        )
        .await
        .expect("REPL completes cleanly");
    }

    #[tokio::test]
    async fn scripted_repl_rejects_unknown_slash() {
        let provider = Arc::new(MockProvider::new("mock", Vec::<String>::new()));
        let state = build_test_state(provider);

        let mut script = ScriptedLines::new(vec!["/whatever".to_string(), "/exit".to_string()]);

        run_with_source(
            state,
            Args {
                session: None,
                scenario: None,
                sample: None,
                mode: None,
                admin: false,
            },
            &mut script,
        )
        .await
        .expect("REPL completes cleanly");
    }
}
