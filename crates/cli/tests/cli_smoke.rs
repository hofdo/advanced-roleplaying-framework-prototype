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

fn run_cli(args: &[&str]) -> Output {
    let exe = std::env::var("CARGO_BIN_EXE_rp").expect("rp test binary path");
    std::process::Command::new(exe)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("cli process should run")
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
