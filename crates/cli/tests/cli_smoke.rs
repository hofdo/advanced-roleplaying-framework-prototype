//! End-to-end smoke test: build a CliState in-process and drive a full
//! scenario → session → turn → world cycle against the in-memory store and a
//! MockProvider. Verifies the CLI's component wiring without spawning the
//! binary.

use std::sync::Arc;

use domain::{TurnMode, ViewerContext};
use engine::{
    BasicFrontendStateProjector, DefaultTurnPipeline, FrontendStateProjector,
    InMemorySessionTurnLock, SessionTurnLock, StreamTurnEvent, StreamTurnRequest, TurnRequestInput,
    stream_turn,
};
use futures::StreamExt;
use persistence::{ApplicationStore, InMemoryApplicationStore};
use providers::{LlmProvider, MockProvider};

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
            // For process_turn: combined player_response + delta JSON
            format!(
                r#"{{
                    "player_response": "The examiner watches carefully.",
                    "world_state_delta": {DELTA_JSON}
                }}"#
            ),
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
    let secrets_visible = serde_json::to_string(&projected).unwrap().contains("haunted");
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
