use crate::{
    BasicContextBuilder, BasicDeltaValidator, BasicFrontendStateProjector,
    BasicHiddenReasoningStripper, BasicPromptBuilder, BasicReasoningStyleOptimizer,
    BasicRoleIdentityActivator, BasicWorldStateReducer, BuildContextInput, ContextBuilder,
    DeltaValidationError, DeltaValidator, FrontendStateProjector, HiddenReasoningStripper,
    InMemorySessionTurnLock, JsonResponseParser, PromptBuilder, ReasoningStyleOptimizer,
    ResponseParser, RoleIdentityActivator, RuleBasedSceneClassifier, SceneClassifier,
    SessionTurnLock, TurnLockError, ValidatedWorldStateDelta, WorldStateReducer,
};
use async_trait::async_trait;
use domain::{
    EntityRef, FrontendStatePatch, MessageRecord, MessageRole, Scenario, SceneReasoningStyle,
    SessionId, TurnMode, ViewerContext, WorldState,
};
use providers::{LlmProvider, ProviderError};
use std::sync::Arc;
use thiserror::Error;
use tracing::instrument;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TurnRequestInput {
    pub session_id: SessionId,
    pub input: String,
    pub mode: Option<TurnMode>,
    pub viewer: ViewerContext,
}

#[derive(Debug, Clone)]
pub struct TurnResponse {
    pub message_id: Uuid,
    pub player_response: String,
    pub scene_type: SceneReasoningStyle,
    pub world_state_version: i64,
    pub changed_entities: Vec<EntityRef>,
    pub frontend_state_patch: FrontendStatePatch,
}

#[derive(Debug, Clone)]
pub struct LoadedTurnState {
    pub scenario: Scenario,
    pub world_state: WorldState,
    pub recent_messages: Vec<MessageRecord>,
}

#[async_trait]
pub trait TurnStateStore: Send + Sync {
    async fn load_turn_state(
        &self,
        session_id: SessionId,
    ) -> Result<LoadedTurnState, TurnPipelineError>;

    async fn persist_successful_turn(
        &self,
        user_message: MessageRecord,
        assistant_message: MessageRecord,
        delta: ValidatedWorldStateDelta,
        updated_state: WorldState,
    ) -> Result<(), TurnPipelineError>;

    async fn persist_error_event(
        &self,
        session_id: SessionId,
        description: String,
    ) -> Result<(), TurnPipelineError>;
}

pub struct DefaultTurnPipeline<P: ?Sized, S: ?Sized, L = InMemorySessionTurnLock> {
    pub provider: Arc<P>,
    pub store: Arc<S>,
    pub turn_lock: L,
    pub scene_classifier: RuleBasedSceneClassifier,
    pub role_activator: BasicRoleIdentityActivator,
    pub reasoning_optimizer: BasicReasoningStyleOptimizer,
    pub context_builder: BasicContextBuilder,
    pub prompt_builder: BasicPromptBuilder,
    pub parser: JsonResponseParser,
    pub stripper: BasicHiddenReasoningStripper,
    pub validator: BasicDeltaValidator,
    pub reducer: BasicWorldStateReducer,
    pub projector: BasicFrontendStateProjector,
}

impl<P: ?Sized, S: ?Sized> DefaultTurnPipeline<P, S, InMemorySessionTurnLock> {
    pub fn new(provider: Arc<P>, store: Arc<S>) -> Self {
        Self::with_lock(provider, store, InMemorySessionTurnLock::default())
    }
}

impl<P: ?Sized, S: ?Sized, L> DefaultTurnPipeline<P, S, L> {
    pub fn with_lock(provider: Arc<P>, store: Arc<S>, turn_lock: L) -> Self {
        Self {
            provider,
            store,
            turn_lock,
            scene_classifier: RuleBasedSceneClassifier,
            role_activator: BasicRoleIdentityActivator,
            reasoning_optimizer: BasicReasoningStyleOptimizer,
            context_builder: BasicContextBuilder,
            prompt_builder: BasicPromptBuilder,
            parser: JsonResponseParser,
            stripper: BasicHiddenReasoningStripper,
            validator: BasicDeltaValidator,
            reducer: BasicWorldStateReducer,
            projector: BasicFrontendStateProjector,
        }
    }
}

impl<P: ?Sized, S: ?Sized, L> DefaultTurnPipeline<P, S, L>
where
    P: LlmProvider + 'static,
    S: TurnStateStore + 'static,
    L: SessionTurnLock,
{
    #[instrument(skip_all, fields(session_id = %request.session_id))]
    pub async fn process_turn(
        &self,
        request: TurnRequestInput,
    ) -> Result<TurnResponse, TurnPipelineError> {
        tracing::info!("turn_started");
        let _guard = self.turn_lock.acquire(request.session_id).await?;
        tracing::info!("turn_lock_acquired");

        let loaded = self.store.load_turn_state(request.session_id).await?;
        let scene_type = self
            .scene_classifier
            .classify(&request.input, &loaded.world_state);
        let active_role =
            self.role_activator
                .activate(&loaded.scenario, &loaded.world_state, scene_type);
        let scene_directive = self.reasoning_optimizer.directive_for(scene_type);
        let context = self.context_builder.build(BuildContextInput {
            scenario: &loaded.scenario,
            world_state: &loaded.world_state,
            active_role,
            scene_directive,
            recent_messages: loaded
                .recent_messages
                .iter()
                .map(|message| crate::MessageContext {
                    role: format!("{:?}", message.role),
                    content: message.content.clone(),
                })
                .collect(),
        });
        tracing::info!("context_built");

        let prompt = self
            .prompt_builder
            .build_non_streaming_prompt(&context, &request.input);
        tracing::info!("provider_called");
        let provider_response = self.provider.generate(prompt).await?;
        let output = match self.parser.parse_turn_output(&provider_response.text) {
            Ok(output) => output,
            Err(error) => {
                self.store
                    .persist_error_event(request.session_id, error.to_string())
                    .await?;
                return Err(error.into());
            }
        };
        let player_response = self.stripper.strip(&output.player_response);
        let validated_delta = self.validator.validate(
            &loaded.scenario,
            &loaded.world_state,
            &output.world_state_delta,
        )?;
        let updated_state = self
            .reducer
            .apply(loaded.world_state.clone(), validated_delta.clone());
        tracing::info!("delta_applied");
        let frontend_state_patch = self.projector.patch_from_delta(
            &loaded.scenario,
            &updated_state,
            &validated_delta,
            &request.viewer,
        );
        tracing::info!("frontend_state_projected");

        let user_message = MessageRecord {
            id: Uuid::new_v4(),
            session_id: request.session_id,
            role: MessageRole::User,
            speaker_id: None,
            content: request.input,
            scene_type: Some(scene_type),
            prompt_template_version: None,
            raw_provider_output: None,
        };
        let assistant_message = MessageRecord {
            id: Uuid::new_v4(),
            session_id: request.session_id,
            role: MessageRole::Assistant,
            speaker_id: loaded.world_state.active_speaker_id.clone(),
            content: player_response.clone(),
            scene_type: Some(scene_type),
            prompt_template_version: Some(crate::PROMPT_TEMPLATE_VERSION.into()),
            raw_provider_output: None,
        };
        let message_id = assistant_message.id;

        self.store
            .persist_successful_turn(
                user_message,
                assistant_message,
                validated_delta,
                updated_state.clone(),
            )
            .await?;

        tracing::info!("turn_finished");
        Ok(TurnResponse {
            message_id,
            player_response,
            scene_type,
            world_state_version: updated_state.version,
            changed_entities: frontend_state_patch.changed_entities.clone(),
            frontend_state_patch,
        })
    }
}

#[derive(Debug, Error)]
pub enum TurnPipelineError {
    #[error("not found")]
    NotFound,
    #[error("turn lock error: {0}")]
    Lock(#[from] TurnLockError),
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
    #[error("parse error: {0}")]
    Parse(#[from] crate::ParseError),
    #[error("delta validation error: {0}")]
    DeltaValidation(#[from] DeltaValidationError),
    #[error("store error: {0}")]
    Store(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::*;
    use providers::MockProvider;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct MemoryTurnStore {
        loaded: LoadedTurnState,
        persisted_state: Mutex<Option<WorldState>>,
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
            _user_message: MessageRecord,
            _assistant_message: MessageRecord,
            _delta: ValidatedWorldStateDelta,
            updated_state: WorldState,
        ) -> Result<(), TurnPipelineError> {
            *self.persisted_state.lock().expect("state mutex") = Some(updated_state);
            Ok(())
        }

        async fn persist_error_event(
            &self,
            _session_id: SessionId,
            _description: String,
        ) -> Result<(), TurnPipelineError> {
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

    fn world_state(session_id: SessionId, scenario_id: domain::ScenarioId) -> WorldState {
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
            }],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        }
    }

    #[tokio::test]
    async fn non_streaming_turn_applies_valid_delta_and_projects_state() {
        let session_id = Uuid::new_v4();
        let scenario = scenario();
        let store = Arc::new(MemoryTurnStore {
            loaded: LoadedTurnState {
                world_state: world_state(session_id, scenario.id),
                scenario,
                recent_messages: vec![],
            },
            persisted_state: Mutex::new(None),
        });
        let raw = r#"{
            "player_response": "The guildhall falls silent.",
            "world_state_delta": {
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
            }
        }"#;
        let provider = Arc::new(MockProvider::new("mock", [raw.into()]));
        let pipeline = DefaultTurnPipeline::new(provider, Arc::clone(&store));

        let response = pipeline
            .process_turn(TurnRequestInput {
                session_id,
                input: "I flood the guildhall with mana.".into(),
                mode: Some(TurnMode::Action),
                viewer: ViewerContext::player(),
            })
            .await
            .expect("turn response");

        assert_eq!(response.player_response, "The guildhall falls silent.");
        assert_eq!(response.world_state_version, 1);
        assert!(
            response
                .changed_entities
                .iter()
                .any(|entity| entity.entity_type == "faction" && entity.id == "guild")
        );
        let persisted = store
            .persisted_state
            .lock()
            .expect("state mutex")
            .clone()
            .expect("persisted state");
        assert_eq!(persisted.factions[0].standing, -5);
        assert_eq!(persisted.clocks[0].current, 2);
    }

    #[tokio::test]
    async fn overpowered_player_fixture_preserves_external_stakes_and_hides_secrets() {
        let session_id = Uuid::new_v4();
        let mut scenario = scenario();
        scenario.secrets = vec![Secret {
            id: "void-mark".into(),
            text: "The player's soul-mark was not created by the goddess.".into(),
            reveal_conditions: vec!["a divine relic reacts to the mark".into()],
        }];
        let mut initial_state = world_state(session_id, scenario.id);
        initial_state.facts.push(Fact {
            id: "void-mark".into(),
            text: "The player's soul-mark was not created by the goddess.".into(),
            visibility: FactVisibility::GmOnly,
            known_by: vec![],
            source: FactSource::Scenario,
            reveal_conditions: vec!["a divine relic reacts to the mark".into()],
        });
        let store = Arc::new(MemoryTurnStore {
            loaded: LoadedTurnState {
                world_state: initial_state,
                scenario,
                recent_messages: vec![],
            },
            persisted_state: Mutex::new(None),
        });
        let raw = r#"{
            "player_response": "You remain unharmed, but the guildhall erupts into alarm as examiners shield civilians and runners bolt for senior officials.",
            "world_state_delta": {
                "facts_to_add": [],
                "npc_changes": [],
                "faction_changes": [
                    {"type":"standing_changed","faction_id":"guild","standing_delta":-5,"reason":"The display caused panic and forced the guild to treat the player as a public risk."}
                ],
                "quest_changes": [],
                "clock_changes": [
                    {"type":"advanced","clock_id":"fame","delta":1,"reason":"Multiple witnesses saw impossible mana flood the guildhall."}
                ],
                "relationship_changes": [],
                "location_change": null,
                "event_log_entries": ["The player revealed abnormal mana during guild registration."]
            }
        }"#;
        let provider = Arc::new(MockProvider::new("mock", [raw.into()]));
        let pipeline = DefaultTurnPipeline::new(provider, Arc::clone(&store));

        let response = pipeline
            .process_turn(TurnRequestInput {
                session_id,
                input: "I flood the guildhall with infinite mana to prove I am powerful.".into(),
                mode: Some(TurnMode::Action),
                viewer: ViewerContext::player(),
            })
            .await
            .expect("turn response");

        assert!(response.player_response.contains("remain unharmed"));
        assert_eq!(response.world_state_version, 1);
        let visible_facts = response
            .frontend_state_patch
            .visible_state
            .expect("visible state")
            .player_known_facts;
        assert!(
            visible_facts
                .iter()
                .all(|fact| !fact.text.contains("soul-mark was not created"))
        );
        let persisted = store
            .persisted_state
            .lock()
            .expect("state mutex")
            .clone()
            .expect("persisted state");
        assert_eq!(persisted.factions[0].standing, -5);
        assert_eq!(persisted.clocks[0].current, 2);
    }
}
