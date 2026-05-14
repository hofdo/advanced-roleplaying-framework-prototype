use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domain::{
    ClockState, Fact, FactSource, FactVisibility, Faction, FactionIdentity, FactionState, Location,
    MessageRecord, Scenario, ScenarioId, ScenarioType, Secret, SessionId, TurnMode, ViewerContext,
    WorldState,
};
use engine::{
    DefaultTurnPipeline, LoadedTurnState, TurnPipelineError, TurnRequestInput, TurnStateStore,
    ValidatedWorldStateDelta,
};
use providers::{LlmRequest, RecordingMockProvider};
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

fn scenario_with_secret() -> Scenario {
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
        secrets: vec![Secret {
            id: "void-mark".into(),
            text: "The soul-mark was not created by the goddess.".into(),
            reveal_conditions: vec!["a divine relic reacts to the mark".into()],
        }],
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
        facts: vec![Fact {
            id: "void-mark".into(),
            text: "The soul-mark was not created by the goddess.".into(),
            visibility: FactVisibility::GmOnly,
            known_by: vec![],
            source: FactSource::Scenario,
            reveal_conditions: vec!["a divine relic reacts to the mark".into()],
            related_secret_ids: vec![],
            reveal_condition_satisfied: None,
        }],
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
        summary: None,
        recent_events: vec![],
    }
}

fn joined_request_text(request: &LlmRequest) -> String {
    request
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

const EMPTY_DELTA_JSON: &str = r#"{
    "facts_to_add": [],
    "npc_changes": [],
    "faction_changes": [],
    "quest_changes": [],
    "clock_changes": [],
    "relationship_changes": [],
    "inventory_changes": [],
    "location_change": null,
    "summary_update": null,
    "event_log_entries": []
}"#;

#[tokio::test]
async fn non_streaming_turn_splits_visible_and_delta_calls() {
    let session_id = Uuid::new_v4();
    let scenario = scenario_with_secret();
    let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
        world_state: world_state(session_id, scenario.id),
        scenario,
        recent_messages: vec![],
    }));

    let provider = Arc::new(RecordingMockProvider::new(
        "mock",
        [
            "The examiner watches you without lowering her hand from the alarm bell.".into(),
            EMPTY_DELTA_JSON.into(),
        ],
    ));
    let recorded = provider.requests();
    let pipeline = DefaultTurnPipeline::new(Arc::clone(&provider), Arc::clone(&store));

    pipeline
        .process_turn(TurnRequestInput {
            session_id,
            input: "I approach the examiner and demand the truth about the soul-mark.".into(),
            mode: Some(TurnMode::Action),
            viewer: ViewerContext::player(),
        })
        .await
        .expect("turn response");

    let requests = recorded.lock().expect("requests mutex");
    assert_eq!(
        requests.len(),
        2,
        "non-streaming turn must call provider twice (visible then delta)"
    );
    assert!(
        !joined_request_text(&requests[0]).contains("The soul-mark was not created by the goddess."),
        "first (visible) request must not contain GM-only fact"
    );
    assert!(
        joined_request_text(&requests[1]).contains("The soul-mark was not created by the goddess."),
        "second (delta-extraction) request must contain GM-only fact"
    );
    assert!(
        !requests[0].json_mode,
        "first (visible) request must not be JSON-mode"
    );
    assert!(
        requests[1].json_mode,
        "second (delta-extraction) request must be JSON-mode"
    );
}

#[tokio::test]
async fn non_streaming_debug_turn_also_splits_calls() {
    let session_id = Uuid::new_v4();
    let scenario = scenario_with_secret();
    let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
        world_state: world_state(session_id, scenario.id),
        scenario,
        recent_messages: vec![],
    }));

    let provider = Arc::new(RecordingMockProvider::new(
        "mock",
        [
            "She nods once and lowers her hand.".into(),
            EMPTY_DELTA_JSON.into(),
        ],
    ));
    let recorded = provider.requests();
    let pipeline = DefaultTurnPipeline::new(Arc::clone(&provider), Arc::clone(&store));

    let debug = pipeline
        .process_turn_debug(TurnRequestInput {
            session_id,
            input: "I ask the examiner what the soul-mark really means.".into(),
            mode: Some(TurnMode::Action),
            viewer: ViewerContext::player(),
        })
        .await
        .expect("debug turn response");

    assert_eq!(
        debug.turn.player_response,
        "She nods once and lowers her hand."
    );
    let requests = recorded.lock().expect("requests mutex");
    assert_eq!(requests.len(), 2);
    assert!(
        !joined_request_text(&requests[0]).contains("The soul-mark was not created by the goddess.")
    );
    assert!(
        joined_request_text(&requests[1]).contains("The soul-mark was not created by the goddess.")
    );
}
