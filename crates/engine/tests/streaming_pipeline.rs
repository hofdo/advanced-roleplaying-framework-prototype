use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domain::{
    ClockState, Faction, FactionIdentity, FactionState, Location, MessageRecord, Scenario,
    ScenarioId, ScenarioType, SessionId, TurnMode, ViewerContext, WorldState,
};
use engine::{
    DefaultTurnPipeline, LoadedTurnState, StreamTurnEvent, StreamTurnRequest, TurnPipelineError,
    TurnStateStore, ValidatedWorldStateDelta, stream_turn,
};
use futures::StreamExt;
use providers::MockProvider;
use uuid::Uuid;

#[derive(Debug)]
struct MemoryTurnStore {
    loaded: LoadedTurnState,
    persisted_state: Mutex<Option<WorldState>>,
    persisted_messages: Mutex<Vec<MessageRecord>>,
    persisted_events: Mutex<Vec<(String, String)>>,
}

impl MemoryTurnStore {
    fn new(loaded: LoadedTurnState) -> Self {
        Self {
            loaded,
            persisted_state: Mutex::new(None),
            persisted_messages: Mutex::new(vec![]),
            persisted_events: Mutex::new(vec![]),
        }
    }
}

#[async_trait]
impl TurnStateStore for MemoryTurnStore {
    async fn load_turn_state(
        &self,
        _session_id: SessionId,
    ) -> Result<LoadedTurnState, TurnPipelineError> {
        Ok(self.loaded.clone())
    }

    async fn persist_successful_turn(
        &self,
        user_message: MessageRecord,
        assistant_message: MessageRecord,
        _delta: ValidatedWorldStateDelta,
        updated_state: WorldState,
    ) -> Result<(), TurnPipelineError> {
        *self.persisted_state.lock().expect("state mutex") = Some(updated_state);
        let mut msgs = self.persisted_messages.lock().expect("messages mutex");
        msgs.push(user_message);
        msgs.push(assistant_message);
        Ok(())
    }

    async fn persist_error_event(
        &self,
        _session_id: SessionId,
        description: String,
    ) -> Result<(), TurnPipelineError> {
        self.persisted_events
            .lock()
            .expect("events mutex")
            .push(("turn_error".into(), description));
        Ok(())
    }

    async fn persist_pipeline_event(
        &self,
        _session_id: SessionId,
        event_type: &'static str,
        description: String,
    ) -> Result<(), TurnPipelineError> {
        self.persisted_events
            .lock()
            .expect("events mutex")
            .push((event_type.into(), description));
        Ok(())
    }
}

fn scenario() -> Scenario {
    Scenario {
        id: Uuid::new_v4(),
        title: "Aurethia".into(),
        scenario_type: ScenarioType::Adventure,
        setting: "high fantasy".into(),
        tone: "heroic".into(),
        rules: vec![],
        locations: vec![Location {
            id: "guildhall".into(),
            name: "Guildhall".into(),
            description: "A hall.".into(),
            visible: true,
        }],
        factions: vec![Faction {
            id: "guild".into(),
            name: "Guild".into(),
            description: "Adventurers.".into(),
            faction_identity: FactionIdentity {
                public_goal: "assign quests".into(),
                hidden_goal: None,
                values: vec![],
                fears: vec![],
                methods: vec![],
            },
            initial_standing: 0,
        }],
        npcs: vec![],
        quests: vec![],
        secrets: vec![],
        clocks: vec![],
    }
}

fn world_state(session_id: SessionId, scenario_id: ScenarioId) -> WorldState {
    WorldState {
        session_id,
        scenario_id,
        version: 0,
        current_location_id: Some("guildhall".into()),
        current_scene: None,
        active_speaker_id: None,
        facts: vec![],
        npcs: vec![],
        factions: vec![FactionState {
            faction_id: "guild".into(),
            standing: 0,
            public_notes: vec![],
            hidden_notes: vec![],
            revealed_goals: vec![],
        }],
        quests: vec![],
        clocks: vec![ClockState {
            id: "fame".into(),
            title: "Fame spreads".into(),
            current: 1,
            max: 6,
            consequence: "Factions notice.".into(),
            visible_to_player: true,
        }],
        relationships: vec![],
        inventory: vec![],
        memories: vec![],
        summary: None,
        recent_events: vec![],
    }
}

fn fixture(session_id: SessionId) -> Arc<MemoryTurnStore> {
    let scenario = scenario();
    Arc::new(MemoryTurnStore::new(LoadedTurnState {
        world_state: world_state(session_id, scenario.id),
        scenario,
        recent_messages: vec![],
    }))
}

const DELTA_JSON: &str = r#"{
    "facts_to_add": [],
    "npc_changes": [],
    "faction_changes": [
        {"type":"standing_changed","faction_id":"guild","standing_delta":-5,"reason":"The player caused panic."}
    ],
    "quest_changes": [],
    "clock_changes": [
        {"type":"advanced","clock_id":"fame","delta":1,"reason":"Many witnesses saw the mana surge."}
    ],
    "relationship_changes": [],
    "location_change": null,
    "event_log_entries": ["The player revealed abnormal mana."]
}"#;

#[tokio::test]
async fn stream_turn_yields_tokens_in_order_then_final() {
    let session_id = Uuid::new_v4();
    let store = fixture(session_id);
    let provider = Arc::new(MockProvider::new(
        "mock",
        ["The guildhall falls silent.".into(), DELTA_JSON.into()],
    ));
    let pipeline = Arc::new(DefaultTurnPipeline::new(provider, Arc::clone(&store)));

    let stream = stream_turn(
        Arc::clone(&pipeline),
        StreamTurnRequest {
            session_id,
            input: "I flood the guildhall with mana.".into(),
            mode: Some(TurnMode::Action),
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

    assert!(!tokens.is_empty(), "must emit at least one token");
    let combined = tokens.join("");
    assert!(combined.contains("guildhall"));
    let final_ = final_event.expect("stream must end with Final");
    assert_eq!(final_.world_state_version, 1);
}

#[tokio::test]
async fn stream_turn_strips_hidden_reasoning_tokens() {
    let session_id = Uuid::new_v4();
    let store = fixture(session_id);
    let provider = Arc::new(MockProvider::new(
        "mock",
        [
            // MockProvider splits on whitespace; embed a `<think>` token to be filtered.
            "Open <think>secretly plotting</think> end.".into(),
            DELTA_JSON.into(),
        ],
    ));
    let pipeline = Arc::new(DefaultTurnPipeline::new(provider, Arc::clone(&store)));

    let stream = stream_turn(
        Arc::clone(&pipeline),
        StreamTurnRequest {
            session_id,
            input: "test".into(),
            mode: Some(TurnMode::Action),
            viewer: ViewerContext::player(),
        },
    );
    futures::pin_mut!(stream);

    let mut tokens = Vec::new();
    while let Some(event) = stream.next().await {
        if let StreamTurnEvent::Token(token) = event.expect("stream event") {
            tokens.push(token);
        }
    }
    let combined = tokens.join("");
    // The streaming-token filter strips any token that contains the opening
    // `<think>` marker. Closing tags and the inner content of unmarked tokens
    // are handled by `BasicHiddenReasoningStripper` when constructing the
    // visible_response that becomes the assistant message — that's a separate
    // guarantee verified elsewhere.
    assert!(
        !combined.contains("<think>"),
        "open reasoning marker must be filtered, got: {combined}"
    );
}

#[tokio::test]
async fn stream_turn_persists_pipeline_milestone_events() {
    let session_id = Uuid::new_v4();
    let store = fixture(session_id);
    let provider = Arc::new(MockProvider::new(
        "mock",
        ["The hall falls silent.".into(), DELTA_JSON.into()],
    ));
    let pipeline = Arc::new(DefaultTurnPipeline::new(provider, Arc::clone(&store)));

    let stream = stream_turn(
        Arc::clone(&pipeline),
        StreamTurnRequest {
            session_id,
            input: "test".into(),
            mode: Some(TurnMode::Action),
            viewer: ViewerContext::player(),
        },
    );
    futures::pin_mut!(stream);
    while let Some(event) = stream.next().await {
        event.expect("stream event");
    }

    let events = store.persisted_events.lock().expect("events mutex");
    let types: Vec<_> = events.iter().map(|(t, _)| t.as_str()).collect();
    for expected in [
        "turn_started",
        "turn_lock_acquired",
        "context_built",
        "provider_called",
        "provider_responded",
        "delta_applied",
        "frontend_state_projected",
        "turn_finished",
        "turn_lock_releasing",
    ] {
        assert!(
            types.contains(&expected),
            "missing pipeline event {expected}; got {types:?}"
        );
    }
}

#[tokio::test]
async fn stream_turn_increments_world_state_version() {
    let session_id = Uuid::new_v4();
    let store = fixture(session_id);
    let provider = Arc::new(MockProvider::new(
        "mock",
        ["narration tokens here".into(), DELTA_JSON.into()],
    ));
    let pipeline = Arc::new(DefaultTurnPipeline::new(provider, Arc::clone(&store)));

    let stream = stream_turn(
        Arc::clone(&pipeline),
        StreamTurnRequest {
            session_id,
            input: "test".into(),
            mode: Some(TurnMode::Action),
            viewer: ViewerContext::player(),
        },
    );
    futures::pin_mut!(stream);
    while let Some(event) = stream.next().await {
        event.expect("stream event");
    }

    let persisted = store
        .persisted_state
        .lock()
        .expect("state mutex")
        .clone()
        .expect("persisted state");
    assert_eq!(persisted.version, 1);
    assert_eq!(persisted.factions[0].standing, -5);
}

#[tokio::test]
async fn stream_turn_propagates_provider_failure_on_delta_extraction() {
    let session_id = Uuid::new_v4();
    let store = fixture(session_id);
    // Only one queued response: streaming narration. The second-pass
    // delta-extraction `generate()` call will hit NoMockResponse.
    let provider = Arc::new(MockProvider::new("mock", ["narration only".into()]));
    let pipeline = Arc::new(DefaultTurnPipeline::new(provider, Arc::clone(&store)));

    let stream = stream_turn(
        Arc::clone(&pipeline),
        StreamTurnRequest {
            session_id,
            input: "test".into(),
            mode: Some(TurnMode::Action),
            viewer: ViewerContext::player(),
        },
    );
    futures::pin_mut!(stream);

    let mut saw_error = false;
    while let Some(event) = stream.next().await {
        if event.is_err() {
            saw_error = true;
            break;
        }
    }
    assert!(saw_error, "stream must surface delta-extraction failure");
}
