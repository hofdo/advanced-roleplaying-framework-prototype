//! End-to-end smoke tests for both in-process wiring and subprocess command
//! behavior.

use std::{path::PathBuf, process::Output, sync::Arc};

use domain::{TurnMode, ViewerContext};
use engine::{
    BasicFrontendStateProjector, DefaultTurnPipeline, FrontendStateProjector,
    InMemorySessionTurnLock, SessionTurnLock, StreamTurnEvent, StreamTurnRequest, TurnRequestInput,
    stream_turn,
};
use futures::StreamExt;
use persistence::{ApplicationStore, InMemoryApplicationStore};
use providers::{LlmProvider, MockProvider};
use shared::build_replay_fixture_draft;
use uuid::Uuid;

const DELTA_JSON: &str = r#"{
    "facts_to_add": [],
    "npc_changes": [],
    "faction_changes": [],
    "quest_changes": [],
    "clock_changes": [
        {"type":"advanced","clock_id":"fame","delta":1,"reason":"witnesses talk"}
    ],
    "relationship_changes": [],
    "location_change": null,
    "event_log_entries": ["The examiner notes the player."]
}"#;

struct TempScenarioFile {
    path: PathBuf,
}

impl TempScenarioFile {
    fn write(contents: &str) -> Self {
        let path = std::env::temp_dir().join(format!("rp-cli-smoke-{}.json", Uuid::new_v4()));
        std::fs::write(&path, contents).expect("temp scenario file should write");
        Self { path }
    }
}

impl Drop for TempScenarioFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

struct TempConfigFile {
    path: PathBuf,
}

impl TempConfigFile {
    fn write(contents: &str) -> Self {
        let path = std::env::temp_dir().join(format!("rp-cli-config-{}.toml", Uuid::new_v4()));
        std::fs::write(&path, contents).expect("temp config file should write");
        Self { path }
    }
}

impl Drop for TempConfigFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn run_cli(args: &[&str]) -> Output {
    let exe = std::env::var("CARGO_BIN_EXE_rp").expect("rp test binary path");
    std::process::Command::new(exe)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("cli process should run")
}

fn run_cli_with_config(config: &TempConfigFile, args: &[&str]) -> Output {
    let mut full_args = vec!["--config", config.path.to_str().expect("config path utf-8")];
    full_args.extend_from_slice(args);
    run_cli(&full_args)
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be utf-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be utf-8")
}

fn sample_scenario() -> domain::Scenario {
    domain::Scenario {
        id: uuid::Uuid::new_v4(),
        title: "CLI Smoke".into(),
        scenario_type: domain::ScenarioType::Adventure,
        setting: "test".into(),
        tone: "test".into(),
        rules: vec![],
        locations: vec![domain::Location {
            id: "guildhall".into(),
            name: "Guildhall".into(),
            description: "".into(),
            visible: true,
        }],
        factions: vec![],
        npcs: vec![],
        quests: vec![],
        secrets: vec![domain::Secret {
            id: "shadow".into(),
            text: "the protagonist is haunted".into(),
            reveal_conditions: vec!["a divine relic reacts".into()],
        }],
        clocks: vec![domain::ClockTemplate {
            id: "fame".into(),
            title: "fame".into(),
            current: 0,
            max: 6,
            consequence: "factions notice".into(),
        }],
    }
}

fn test_config_toml(default_name: &str, base_url: &str, model: &str) -> String {
    format!(
        r#"[server]
host = "0.0.0.0"
port = 8080

[database]
url = "postgres://roleplay:roleplay@localhost:5432/roleplay"

[storage]
backend = "memory"
migrate_on_startup = true

[provider.default]
name = "{default_name}"
provider_type = "openai_compatible"
base_url = "{base_url}"
api_key = ""
model = "{model}"
supports_streaming = true
supports_json_mode = true
max_context_tokens = 4096
request_timeout_seconds = 5
stream_idle_timeout_seconds = 5
max_retries = 0
include_usage = true

[admin]
enabled = false

[debug]
store_raw_provider_output = false
allow_debug_state = false
"#
    )
}

fn build_state() -> (
    Arc<dyn ApplicationStore>,
    Arc<dyn SessionTurnLock>,
    Arc<MockProvider>,
) {
    let store: Arc<dyn ApplicationStore> = Arc::new(InMemoryApplicationStore::new(false));
    let lock: Arc<dyn SessionTurnLock> = Arc::new(InMemorySessionTurnLock::default());
    let provider = Arc::new(MockProvider::new(
        "mock",
        [
            // Non-streaming turns now use two provider calls: visible response,
            // then delta extraction JSON.
            "The examiner watches carefully.".into(),
            DELTA_JSON.into(),
        ],
    ));
    (store, lock, provider)
}

#[tokio::test]
async fn full_scenario_session_turn_world_cycle_in_memory() {
    let (store, lock, provider) = build_state();

    let scenario = store
        .create_scenario(sample_scenario())
        .await
        .expect("create scenario");
    let session = store
        .create_session(scenario.id, "smoke".into())
        .await
        .expect("create session")
        .expect("session");

    let provider_arc: Arc<dyn LlmProvider> = provider.clone();
    let pipeline = Arc::new(DefaultTurnPipeline::with_lock(
        provider_arc,
        Arc::clone(&store),
        lock,
    ));

    let response = pipeline
        .process_turn(TurnRequestInput {
            session_id: session.id,
            input: "I greet the examiner.".into(),
            mode: Some(TurnMode::Dialogue),
            viewer: ViewerContext::player(),
        })
        .await
        .expect("process_turn");

    assert_eq!(response.world_state_version, 1);
    assert!(response.player_response.contains("examiner"));

    let world_state = store
        .world_state(session.id)
        .await
        .expect("world state query")
        .expect("world state present");
    assert_eq!(world_state.clocks[0].current, 1);

    let projected =
        BasicFrontendStateProjector.project(&scenario, &world_state, &ViewerContext::player());
    let secrets_visible = serde_json::to_string(&projected)
        .unwrap()
        .contains("haunted");
    assert!(
        !secrets_visible,
        "player projection must not leak GM-only secrets"
    );

    let admin_view = serde_json::to_string(&world_state).unwrap();
    assert!(
        admin_view.contains("haunted"),
        "admin/raw world state must still contain GM-only facts"
    );
}

#[tokio::test]
async fn streaming_turn_emits_tokens_metadata_and_final() {
    let store: Arc<dyn ApplicationStore> = Arc::new(InMemoryApplicationStore::new(false));
    let lock: Arc<dyn SessionTurnLock> = Arc::new(InMemorySessionTurnLock::default());
    let provider = Arc::new(MockProvider::new(
        "mock",
        [
            // First response is whitespace-split into stream tokens.
            "The examiner watches in silence.".into(),
            // Second is the delta-extraction generate() result.
            DELTA_JSON.into(),
        ],
    ));

    let scenario = store
        .create_scenario(sample_scenario())
        .await
        .expect("create scenario");
    let session = store
        .create_session(scenario.id, "stream-smoke".into())
        .await
        .expect("create session")
        .expect("session");

    let provider_arc: Arc<dyn LlmProvider> = provider.clone();
    let pipeline = Arc::new(DefaultTurnPipeline::with_lock(
        provider_arc,
        Arc::clone(&store),
        lock,
    ));

    let stream = stream_turn(
        pipeline,
        StreamTurnRequest {
            session_id: session.id,
            input: "I greet the examiner.".into(),
            mode: Some(TurnMode::Dialogue),
            viewer: ViewerContext::player(),
        },
    );
    futures::pin_mut!(stream);

    let mut tokens = Vec::new();
    let mut final_event = None;
    while let Some(event) = stream.next().await {
        match event.expect("stream event") {
            StreamTurnEvent::Token(token) => tokens.push(token),
            StreamTurnEvent::ProviderMetadata(_) => {}
            StreamTurnEvent::Final(final_) => final_event = Some(final_),
        }
    }
    assert!(!tokens.is_empty());
    let final_ = final_event.expect("must receive Final event");
    assert_eq!(final_.world_state_version, 1);
}

#[tokio::test]
async fn session_timeline_tracks_public_and_raw_history() {
    let (store, lock, provider) = build_state();

    let scenario = store
        .create_scenario(sample_scenario())
        .await
        .expect("create scenario");
    let session = store
        .create_session(scenario.id, "timeline-smoke".into())
        .await
        .expect("create session")
        .expect("session");

    let provider_arc: Arc<dyn LlmProvider> = provider.clone();
    let pipeline = Arc::new(DefaultTurnPipeline::with_lock(
        provider_arc,
        Arc::clone(&store),
        lock,
    ));

    let response = pipeline
        .process_turn(TurnRequestInput {
            session_id: session.id,
            input: "I greet the examiner.".into(),
            mode: Some(TurnMode::Dialogue),
            viewer: ViewerContext::player(),
        })
        .await
        .expect("process_turn");

    assert_eq!(response.world_state_version, 1);

    let timeline = store.timeline(session.id).await.expect("public timeline");
    let kinds = timeline.iter().map(|entry| entry.kind.as_str()).collect::<Vec<_>>();
    let user_index = kinds
        .iter()
        .position(|kind| *kind == "user_message")
        .expect("user message entry");
    let assistant_index = kinds
        .iter()
        .position(|kind| *kind == "assistant_message")
        .expect("assistant message entry");
    let world_event_index = kinds
        .iter()
        .position(|kind| *kind == "world_event")
        .expect("world event entry");

    assert!(user_index < assistant_index);
    assert!(assistant_index < world_event_index);
    assert!(timeline[assistant_index].description.contains("examiner"));

    let raw_timeline = store
        .raw_timeline(session.id)
        .await
        .expect("raw timeline query")
        .expect("raw timeline");
    assert_eq!(raw_timeline.messages.len(), 2);
    assert!(raw_timeline.deltas.is_empty());
    assert!(!raw_timeline.events.is_empty());
}

#[tokio::test]
async fn export_fixture_draft_matches_schema() {
    let (store, lock, provider) = build_state();

    let scenario = store
        .create_scenario(sample_scenario())
        .await
        .expect("create scenario");
    let session = store
        .create_session(scenario.id, "fixture-draft".into())
        .await
        .expect("create session")
        .expect("session");

    let provider_arc: Arc<dyn LlmProvider> = provider.clone();
    let pipeline = Arc::new(DefaultTurnPipeline::with_lock(
        provider_arc,
        Arc::clone(&store),
        lock,
    ));

    let response = pipeline
        .process_turn(TurnRequestInput {
            session_id: session.id,
            input: "I greet the examiner.".into(),
            mode: Some(TurnMode::Dialogue),
            viewer: ViewerContext::player(),
        })
        .await
        .expect("process_turn");
    assert_eq!(response.world_state_version, 1);

    let world_state = store
        .world_state(session.id)
        .await
        .expect("world state query")
        .expect("world state");
    let visible_state =
        BasicFrontendStateProjector.project(&scenario, &world_state, &ViewerContext::player());
    let fixture = build_replay_fixture_draft(
        "smoke".into(),
        Some(session.id),
        scenario.clone(),
        &world_state,
        &visible_state,
    );
    let fixture_json = serde_json::to_value(&fixture).expect("fixture json");

    assert_eq!(fixture_json["version"], 1);
    assert_eq!(fixture_json["name"], "smoke");
    assert_eq!(fixture_json["source_session_id"], session.id.to_string());
    assert!(fixture_json.get("scenario").is_some());
    assert_eq!(fixture_json["turns"], serde_json::json!([]));
    assert_eq!(fixture_json["expected_final"]["world_state_version"], 1);
}

#[test]
fn scenario_validate_reports_valid_and_invalid_files() {
    let valid = TempScenarioFile::write(include_str!("../scenarios/templates/scenario.template.json"));
    let valid_path = valid.path.to_string_lossy().into_owned();
    let valid_output = run_cli(&["scenario", "validate", "--file", &valid_path]);

    assert!(valid_output.status.success(), "stderr: {}", stderr(&valid_output));
    let valid_stdout = stdout(&valid_output);
    assert!(valid_stdout.contains("valid scenario:"));
    assert!(valid_stdout.contains("title:"));
    assert!(valid_stdout.contains("locations:"));
    assert!(valid_stdout.contains("npcs:"));

    let invalid = TempScenarioFile::write(
        r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "title": "Invalid File Scenario",
            "scenario_type": "adventure",
            "setting": "Test setting",
            "tone": "test",
            "rules": [],
            "locations": [
                {
                    "id": "same",
                    "name": "One",
                    "description": "One",
                    "visible": true
                },
                {
                    "id": "same",
                    "name": "Two",
                    "description": "Two",
                    "visible": true
                }
            ],
            "factions": [],
            "npcs": [],
            "quests": [],
            "secrets": [],
            "clocks": []
        }"#,
    );
    let invalid_path = invalid.path.to_string_lossy().into_owned();
    let invalid_output = run_cli(&["scenario", "validate", "--file", &invalid_path]);

    assert!(!invalid_output.status.success());
    assert!(stderr(&invalid_output).contains("duplicate location id"));
}

#[test]
fn provider_status_reports_default_provider_in_memory_mode() {
    let config = TempConfigFile::write(&test_config_toml("status-default", "", "status-model"));

    let output = run_cli_with_config(&config, &["provider", "status"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let rendered = stdout(&output);
    assert!(rendered.contains("storage: memory"));
    assert!(rendered.contains("default: status-default"));
    assert!(rendered.contains("registered providers: unavailable in memory mode"));
}

#[test]
fn provider_test_default_reports_health_and_readiness() {
    let config = TempConfigFile::write(&test_config_toml("test-default", "", "test-model"));

    let output = run_cli_with_config(&config, &["provider", "test"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let rendered = stdout(&output);
    assert!(rendered.contains("name: test-default"));
    assert!(rendered.contains("health: ok=false"));
    assert!(rendered.contains("readiness: configured=false reachable=false"));
}

#[test]
fn provider_test_registered_requires_postgres_for_provider_id() {
    let config = TempConfigFile::write(&test_config_toml("test-default", "", "test-model"));
    let provider_id = Uuid::new_v4().to_string();

    let output =
        run_cli_with_config(&config, &["provider", "test", "--provider-id", &provider_id]);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("provider lookup by id is unavailable in memory mode"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn scenario_inspect_prints_author_facing_summary() {
    let output = run_cli(&[
        "scenario",
        "inspect",
        "--file",
        "scenarios/samples/bride-of-the-iron-archduke.json",
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let rendered = stdout(&output);
    assert!(rendered.contains("Title: The Bride of the Iron Archduke"));
    assert!(rendered.contains("Opening location:"));
    assert!(rendered.contains("Opening speaker:"));
}

#[test]
fn scenario_samples_lists_builtin_names() {
    let output = run_cli(&["scenario", "samples"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("bride-of-the-iron-archduke"));
}

#[test]
fn scenario_template_prints_valid_json() {
    let output = run_cli(&["scenario", "template"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let scenario: domain::Scenario =
        serde_json::from_str(&stdout(&output)).expect("template json should parse");
    domain::validate_scenario(&scenario).expect("template json should validate");
}
