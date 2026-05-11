use crate::{
    AgentContext, BasicContextBuilder, BasicDeltaValidator, BasicFrontendStateProjector,
    BasicHiddenReasoningStripper, BasicPromptBuilder, BasicReasoningStyleOptimizer,
    BasicRoleIdentityActivator, BasicWorldStateReducer, BuildContextInput, ContextBuilder,
    DeltaValidationError, DeltaValidator, FrontendStateProjector, HiddenReasoningStripper,
    InMemorySessionTurnLock, JsonResponseParser, PromptBuilder, ReasoningStyleOptimizer,
    ResponseParser, RoleIdentityActivator, RuleBasedSceneClassifier, SceneClassifier,
    SessionTurnLock, TurnLockError, ValidatedWorldStateDelta, WorldStateReducer, repair_prompt,
};
use async_trait::async_trait;
use domain::{
    EntityRef, FrontendStatePatch, MessageRecord, MessageRole, Scenario, SceneReasoningStyle,
    SessionId, TurnMode, ViewerContext, WorldState, WorldStateDelta,
};
use providers::{LlmMessage, LlmMessageRole, LlmProvider, LlmRequest, ProviderError};
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
pub struct DebugTurnResponse {
    pub turn: TurnResponse,
    pub applied_delta: WorldStateDelta,
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

    async fn persist_pipeline_event(
        &self,
        session_id: SessionId,
        event_type: &'static str,
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

/// Holds everything needed to start a streaming (or non-streaming) provider call.
/// Produced by [`DefaultTurnPipeline::prepare_turn_context`].
#[derive(Debug, Clone)]
pub struct PreparedTurn {
    /// Loaded DB state for this turn.
    pub loaded: LoadedTurnState,
    /// Built agent context passed to the prompt builder.
    pub context: AgentContext,
    /// Classified scene style.
    pub scene_type: SceneReasoningStyle,
}

/// The post-provider results ready to be persisted.
/// Produced by [`DefaultTurnPipeline::finalize_turn_delta`].
#[derive(Debug, Clone)]
pub struct FinalizedTurn {
    pub user_message: MessageRecord,
    pub assistant_message: MessageRecord,
    pub validated_delta: ValidatedWorldStateDelta,
    pub updated_world_state: WorldState,
    pub world_state_version: i64,
    pub frontend_state_patch: FrontendStatePatch,
    pub visible_response: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineEventKind {
    TurnStarted,
    TurnLockAcquired,
    ContextBuilt,
    ProviderCalled,
    ProviderResponded,
    DeltaApplied,
    FrontendStateProjected,
    TurnFinished,
    TurnLockReleasing,
    ProviderUsageCaptured,
}

impl PipelineEventKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::TurnStarted => "turn_started",
            Self::TurnLockAcquired => "turn_lock_acquired",
            Self::ContextBuilt => "context_built",
            Self::ProviderCalled => "provider_called",
            Self::ProviderResponded => "provider_responded",
            Self::DeltaApplied => "delta_applied",
            Self::FrontendStateProjected => "frontend_state_projected",
            Self::TurnFinished => "turn_finished",
            Self::TurnLockReleasing => "turn_lock_releasing",
            Self::ProviderUsageCaptured => "provider_usage_captured",
        }
    }
}

impl<P: ?Sized, S: ?Sized, L> DefaultTurnPipeline<P, S, L>
where
    S: TurnStateStore + 'static,
{
    /// Load state, classify scene, activate role, build context.
    /// The caller is responsible for holding the turn lock before calling this.
    pub async fn prepare_turn_context(
        &self,
        session_id: SessionId,
        input: &str,
        mode: Option<TurnMode>,
    ) -> Result<PreparedTurn, TurnPipelineError> {
        let loaded = self.store.load_turn_state(session_id).await?;
        let classified = self.scene_classifier.classify(input, &loaded.world_state);
        // TurnMode::Direct and TurnMode::Remember override the classified scene type.
        let scene_type = match mode {
            Some(TurnMode::Direct) => SceneReasoningStyle::RulesAdjudication,
            Some(TurnMode::Remember) => SceneReasoningStyle::WorldSimulation,
            _ => classified,
        };
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
            mode,
        });
        Ok(PreparedTurn {
            loaded,
            context,
            scene_type,
        })
    }

    /// Core finalization: validate a pre-parsed delta, apply it, project the
    /// frontend patch, and build the two message records ready for persistence.
    /// Both the streaming and non-streaming paths converge here so validation,
    /// reduction, and projection logic live in exactly one place.
    pub fn finalize_with_parsed_delta(
        &self,
        session_id: SessionId,
        prepared: &PreparedTurn,
        visible_response: &str,
        delta: domain::WorldStateDelta,
        user_input: &str,
        viewer: &ViewerContext,
    ) -> Result<FinalizedTurn, TurnPipelineError> {
        let validated_delta = self.validator.validate(
            &prepared.loaded.scenario,
            &prepared.loaded.world_state,
            &delta,
        )?;
        let updated_world_state = self
            .reducer
            .apply(prepared.loaded.world_state.clone(), validated_delta.clone());
        let frontend_state_patch = self.projector.patch_from_delta(
            &prepared.loaded.scenario,
            &updated_world_state,
            &validated_delta,
            viewer,
        );
        let visible_response = visible_response.to_owned();
        let world_state_version = updated_world_state.version;
        let user_message = MessageRecord {
            id: Uuid::new_v4(),
            session_id,
            role: MessageRole::User,
            speaker_id: None,
            content: user_input.to_owned(),
            scene_type: Some(prepared.scene_type),
            prompt_template_version: None,
            raw_provider_output: None,
        };
        let assistant_message = MessageRecord {
            id: Uuid::new_v4(),
            session_id,
            role: MessageRole::Assistant,
            speaker_id: prepared.loaded.world_state.active_speaker_id.clone(),
            content: visible_response.clone(),
            scene_type: Some(prepared.scene_type),
            prompt_template_version: Some(crate::PROMPT_TEMPLATE_VERSION.into()),
            raw_provider_output: None,
        };
        Ok(FinalizedTurn {
            user_message,
            assistant_message,
            validated_delta,
            updated_world_state,
            world_state_version,
            frontend_state_patch,
            visible_response,
        })
    }
}

impl<P: ?Sized, S: ?Sized, L> DefaultTurnPipeline<P, S, L>
where
    P: LlmProvider + 'static,
    S: TurnStateStore + 'static,
    L: SessionTurnLock,
{
    async fn record_pipeline_event(
        &self,
        session_id: SessionId,
        event: PipelineEventKind,
    ) -> Result<(), TurnPipelineError> {
        self.store
            .persist_pipeline_event(session_id, event.as_str(), event.as_str().to_owned())
            .await
    }

    /// Parse the delta JSON, validate it, apply it, project the frontend patch,
    /// and build the two message records ready for persistence.
    ///
    /// - `visible_response`: the stripped narration shown to the player
    /// - `raw_delta_text`: JSON string that `parse_delta_output` can decode
    ///   (for streaming: output of the second delta-extraction provider call)
    ///
    /// On parse failure, makes one controlled repair attempt via
    /// `provider.generate()`. If repair also fails, persists an error event
    /// and returns `Err` without mutating world state.
    ///
    /// Does NOT persist a successful turn — the caller calls
    /// `store.persist_successful_turn`.
    pub async fn finalize_turn_delta(
        &self,
        session_id: SessionId,
        prepared: &PreparedTurn,
        visible_response: &str,
        raw_delta_text: &str,
        user_input: &str,
        viewer: &ViewerContext,
    ) -> Result<FinalizedTurn, TurnPipelineError> {
        let delta = match self.parser.parse_delta_output(raw_delta_text) {
            Ok(delta) => delta,
            Err(parse_err) => {
                tracing::warn!("delta parse failed, attempting repair: {parse_err}");
                let repair_request = LlmRequest {
                    messages: vec![LlmMessage {
                        role: LlmMessageRole::User,
                        content: repair_prompt(raw_delta_text),
                    }],
                    temperature: Some(0.2),
                    max_tokens: None,
                    json_mode: true,
                };
                let repaired = self.provider.generate(repair_request).await?;
                match self.parser.parse_delta_output(&repaired.text) {
                    Ok(delta) => {
                        tracing::info!("delta parse succeeded after repair");
                        delta
                    }
                    Err(repair_err) => {
                        let description =
                            format!("delta parse failed after repair attempt: {repair_err}");
                        tracing::error!("{description}");
                        self.store
                            .persist_error_event(session_id, description.clone())
                            .await?;
                        return Err(TurnPipelineError::Parse(repair_err));
                    }
                }
            }
        };
        self.finalize_with_parsed_delta(
            session_id,
            prepared,
            visible_response,
            delta,
            user_input,
            viewer,
        )
    }

    #[instrument(skip_all, fields(session_id = %request.session_id))]
    pub async fn process_turn(
        &self,
        request: TurnRequestInput,
    ) -> Result<TurnResponse, TurnPipelineError> {
        tracing::info!("turn_started");
        self.record_pipeline_event(request.session_id, PipelineEventKind::TurnStarted)
            .await?;
        let _guard = self.turn_lock.acquire(request.session_id).await?;
        tracing::info!("turn_lock_acquired");
        self.record_pipeline_event(request.session_id, PipelineEventKind::TurnLockAcquired)
            .await?;

        // --- Preparation: load state, classify scene, build context ---
        let prepared = self
            .prepare_turn_context(request.session_id, &request.input, request.mode)
            .await?;
        tracing::info!("context_built");
        self.record_pipeline_event(request.session_id, PipelineEventKind::ContextBuilt)
            .await?;

        // --- Non-streaming provider call: emits player_response + delta JSON ---
        let prompt = self
            .prompt_builder
            .build_non_streaming_prompt(&prepared.context, &request.input);
        tracing::info!("provider_called");
        self.record_pipeline_event(request.session_id, PipelineEventKind::ProviderCalled)
            .await?;
        let provider_response = self.provider.generate(prompt).await?;
        self.record_pipeline_event(request.session_id, PipelineEventKind::ProviderResponded)
            .await?;
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

        // --- Finalization: validate delta, reduce, project, build records ---
        let finalized = self.finalize_with_parsed_delta(
            request.session_id,
            &prepared,
            &player_response,
            output.world_state_delta,
            &request.input,
            &request.viewer,
        )?;
        let mut finalized = finalized;
        finalized.assistant_message.raw_provider_output = Some(
            provider_response
                .raw_json
                .unwrap_or_else(|| serde_json::Value::String(provider_response.text)),
        );
        tracing::info!("delta_applied");
        self.record_pipeline_event(request.session_id, PipelineEventKind::DeltaApplied)
            .await?;
        tracing::info!("frontend_state_projected");
        self.record_pipeline_event(
            request.session_id,
            PipelineEventKind::FrontendStateProjected,
        )
        .await?;

        let message_id = finalized.assistant_message.id;
        self.store
            .persist_successful_turn(
                finalized.user_message,
                finalized.assistant_message,
                finalized.validated_delta,
                finalized.updated_world_state,
            )
            .await?;

        tracing::info!("turn_finished");
        self.record_pipeline_event(request.session_id, PipelineEventKind::TurnFinished)
            .await?;
        self.record_pipeline_event(request.session_id, PipelineEventKind::TurnLockReleasing)
            .await?;
        Ok(TurnResponse {
            message_id,
            player_response: finalized.visible_response,
            scene_type: prepared.scene_type,
            world_state_version: finalized.world_state_version,
            changed_entities: finalized.frontend_state_patch.changed_entities.clone(),
            frontend_state_patch: finalized.frontend_state_patch,
        })
    }

    #[instrument(skip_all, fields(session_id = %request.session_id))]
    pub async fn process_turn_debug(
        &self,
        request: TurnRequestInput,
    ) -> Result<DebugTurnResponse, TurnPipelineError> {
        tracing::info!("debug_turn_started");
        self.record_pipeline_event(request.session_id, PipelineEventKind::TurnStarted)
            .await?;
        let _guard = self.turn_lock.acquire(request.session_id).await?;
        self.record_pipeline_event(request.session_id, PipelineEventKind::TurnLockAcquired)
            .await?;

        let prepared = self
            .prepare_turn_context(request.session_id, &request.input, request.mode)
            .await?;
        self.record_pipeline_event(request.session_id, PipelineEventKind::ContextBuilt)
            .await?;

        let prompt = self
            .prompt_builder
            .build_non_streaming_prompt(&prepared.context, &request.input);
        self.record_pipeline_event(request.session_id, PipelineEventKind::ProviderCalled)
            .await?;
        let provider_response = self.provider.generate(prompt).await?;
        self.record_pipeline_event(request.session_id, PipelineEventKind::ProviderResponded)
            .await?;
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
        let raw_delta = output.world_state_delta.clone();

        let finalized = self.finalize_with_parsed_delta(
            request.session_id,
            &prepared,
            &player_response,
            output.world_state_delta,
            &request.input,
            &request.viewer,
        )?;
        let mut finalized = finalized;
        finalized.assistant_message.raw_provider_output = Some(
            provider_response
                .raw_json
                .unwrap_or_else(|| serde_json::Value::String(provider_response.text)),
        );
        self.record_pipeline_event(request.session_id, PipelineEventKind::DeltaApplied)
            .await?;
        self.record_pipeline_event(
            request.session_id,
            PipelineEventKind::FrontendStateProjected,
        )
        .await?;

        let message_id = finalized.assistant_message.id;
        self.store
            .persist_successful_turn(
                finalized.user_message,
                finalized.assistant_message,
                finalized.validated_delta,
                finalized.updated_world_state,
            )
            .await?;

        tracing::info!("debug_turn_finished");
        self.record_pipeline_event(request.session_id, PipelineEventKind::TurnFinished)
            .await?;
        self.record_pipeline_event(request.session_id, PipelineEventKind::TurnLockReleasing)
            .await?;
        Ok(DebugTurnResponse {
            turn: TurnResponse {
                message_id,
                player_response: finalized.visible_response,
                scene_type: prepared.scene_type,
                world_state_version: finalized.world_state_version,
                changed_entities: finalized.frontend_state_patch.changed_entities.clone(),
                frontend_state_patch: finalized.frontend_state_patch,
            },
            applied_delta: raw_delta,
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
                visible_to_player: true,
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
        let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
            world_state: world_state(session_id, scenario.id),
            scenario,
            recent_messages: vec![],
        }));
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
    async fn persisted_assistant_message_records_prompt_template_version() {
        let session_id = Uuid::new_v4();
        let scenario = scenario();
        let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
            world_state: world_state(session_id, scenario.id),
            scenario,
            recent_messages: vec![],
        }));
        let raw = r#"{
            "player_response": "The guildhall falls silent.",
            "world_state_delta": {
                "facts_to_add": [],
                "npc_changes": [],
                "faction_changes": [],
                "quest_changes": [],
                "clock_changes": [],
                "relationship_changes": [],
                "location_change": null,
                "event_log_entries": []
            }
        }"#;
        let provider = Arc::new(MockProvider::new("mock", [raw.into()]));
        let pipeline = DefaultTurnPipeline::new(provider, Arc::clone(&store));

        pipeline
            .process_turn(TurnRequestInput {
                session_id,
                input: "I wait for the room to settle.".into(),
                mode: Some(TurnMode::Action),
                viewer: ViewerContext::player(),
            })
            .await
            .expect("turn response");

        let persisted_messages = store.persisted_messages.lock().expect("messages mutex");
        let assistant_message = persisted_messages
            .iter()
            .find(|message| message.role == MessageRole::Assistant)
            .expect("assistant message");
        assert_eq!(
            assistant_message.prompt_template_version.as_deref(),
            Some(crate::PROMPT_TEMPLATE_VERSION)
        );
    }

    #[tokio::test]
    async fn successful_turn_persists_pipeline_milestone_events() {
        let session_id = Uuid::new_v4();
        let scenario = scenario();
        let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
            world_state: world_state(session_id, scenario.id),
            scenario,
            recent_messages: vec![],
        }));
        let raw = r#"{
            "player_response": "The guildhall falls silent.",
            "world_state_delta": {
                "facts_to_add": [],
                "npc_changes": [],
                "faction_changes": [],
                "quest_changes": [],
                "clock_changes": [],
                "relationship_changes": [],
                "location_change": null,
                "event_log_entries": []
            }
        }"#;
        let provider = Arc::new(MockProvider::new("mock", [raw.into()]));
        let pipeline = DefaultTurnPipeline::new(provider, Arc::clone(&store));

        pipeline
            .process_turn(TurnRequestInput {
                session_id,
                input: "I wait.".into(),
                mode: Some(TurnMode::Action),
                viewer: ViewerContext::player(),
            })
            .await
            .expect("turn response");

        let event_types = store
            .persisted_events
            .lock()
            .expect("events mutex")
            .iter()
            .map(|(event_type, _)| event_type.clone())
            .collect::<Vec<_>>();
        assert!(event_types.contains(&"turn_started".into()));
        assert!(event_types.contains(&"turn_lock_acquired".into()));
        assert!(event_types.contains(&"context_built".into()));
        assert!(event_types.contains(&"provider_called".into()));
        assert!(event_types.contains(&"provider_responded".into()));
        assert!(event_types.contains(&"delta_applied".into()));
        assert!(event_types.contains(&"frontend_state_projected".into()));
        assert!(event_types.contains(&"turn_finished".into()));
        assert!(event_types.contains(&"turn_lock_releasing".into()));
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
            related_secret_ids: vec![],
            reveal_condition_satisfied: None,
        });
        let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
            world_state: initial_state,
            scenario,
            recent_messages: vec![],
        }));
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

    /// The first provider call returns a malformed delta; the repair call
    /// returns valid JSON.  The pipeline must apply the repaired delta and
    /// report a successful turn.
    #[tokio::test]
    async fn repair_retry_succeeds_when_second_call_returns_valid_delta() {
        let session_id = Uuid::new_v4();
        let scenario = scenario();
        let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
            world_state: world_state(session_id, scenario.id),
            scenario,
            recent_messages: vec![],
        }));

        // First response: a valid player_response/world_state_delta wrapper so
        // parse_turn_output succeeds, but wrap the delta part in a separate call
        // that comes out malformed.  To exercise finalize_turn_delta directly,
        // we queue: (1) the full non-streaming turn response (valid), then
        // (2) a repair response for the delta.
        //
        // The easiest way to drive the repair path is to call
        // finalize_turn_delta with raw_delta_text that is bad JSON, so the
        // pipeline issues a second provider.generate() with the repair prompt.
        let valid_delta_json = r#"{
            "facts_to_add": [],
            "npc_changes": [],
            "faction_changes": [],
            "quest_changes": [],
            "clock_changes": [],
            "relationship_changes": [],
            "location_change": null,
            "event_log_entries": ["Repaired successfully."]
        }"#;

        // The non-streaming turn also needs a valid response so process_turn
        // doesn't fail before reaching finalize_turn_delta.  We drive
        // finalize_turn_delta directly here to keep the test focused.
        let provider = Arc::new(MockProvider::new("mock", [valid_delta_json.into()]));
        let pipeline = DefaultTurnPipeline::new(Arc::clone(&provider), Arc::clone(&store));

        let prepared = pipeline
            .prepare_turn_context(session_id, "test input", None)
            .await
            .expect("prepared");

        // Pass bad JSON as raw_delta_text — triggers the repair path.
        let finalized = pipeline
            .finalize_turn_delta(
                session_id,
                &prepared,
                "visible narration",
                "{ this is not valid json !!!",
                "test input",
                &ViewerContext::player(),
            )
            .await
            .expect("finalize with repair");

        assert!(
            finalized
                .validated_delta
                .0
                .event_log_entries
                .contains(&"Repaired successfully.".to_owned())
        );
    }

    /// Both the initial parse and the repair call return bad JSON.  The
    /// pipeline must persist an error event and return `Err` without touching
    /// the world state.
    #[tokio::test]
    async fn repair_retry_persists_error_event_when_repair_also_fails() {
        let session_id = Uuid::new_v4();
        let scenario = scenario();
        let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
            world_state: world_state(session_id, scenario.id),
            scenario,
            recent_messages: vec![],
        }));

        // The repair call also returns invalid JSON.
        let provider = Arc::new(MockProvider::new("mock", ["not json either".into()]));
        let pipeline = DefaultTurnPipeline::new(Arc::clone(&provider), Arc::clone(&store));

        let prepared = pipeline
            .prepare_turn_context(session_id, "test input", None)
            .await
            .expect("prepared");

        let result = pipeline
            .finalize_turn_delta(
                session_id,
                &prepared,
                "visible narration",
                "{ also bad json }",
                "test input",
                &ViewerContext::player(),
            )
            .await;

        assert!(
            result.is_err(),
            "expected Err when both initial parse and repair fail"
        );
        // World state must NOT have been persisted.
        assert!(
            store.persisted_state.lock().expect("state mutex").is_none(),
            "world state must not be mutated on double parse failure"
        );
    }

    #[tokio::test]
    async fn debug_turn_returns_applied_delta_with_faction_changes() {
        let session_id = Uuid::new_v4();
        let scenario = scenario();
        let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
            world_state: world_state(session_id, scenario.id),
            scenario,
            recent_messages: vec![],
        }));
        let raw = r#"{
            "player_response": "The examiner nods cautiously.",
            "world_state_delta": {
                "facts_to_add": [],
                "npc_changes": [],
                "faction_changes": [{"type":"standing_changed","faction_id":"guild","standing_delta":-3,"reason":"Suspicious behavior."}],
                "quest_changes": [],
                "clock_changes": [],
                "relationship_changes": [],
                "location_change": null,
                "event_log_entries": []
            }
        }"#;
        let provider = Arc::new(MockProvider::new("mock", [raw.into()]));
        let pipeline = DefaultTurnPipeline::new(provider, Arc::clone(&store));

        let result = pipeline
            .process_turn_debug(TurnRequestInput {
                session_id,
                input: "I act suspiciously.".into(),
                mode: Some(TurnMode::Action),
                viewer: ViewerContext::player(),
            })
            .await
            .unwrap();

        assert_eq!(result.turn.world_state_version, 1);
        assert_eq!(result.applied_delta.faction_changes.len(), 1);
        match &result.applied_delta.faction_changes[0] {
            FactionChange::StandingChanged { faction_id, .. } => {
                assert_eq!(faction_id, "guild");
            }
            other => panic!("unexpected faction change variant: {:?}", other),
        }
    }

    #[tokio::test]
    async fn debug_turn_persists_state_same_as_regular_turn() {
        let session_id = Uuid::new_v4();
        let scenario = scenario();
        let store = Arc::new(MemoryTurnStore::new(LoadedTurnState {
            world_state: world_state(session_id, scenario.id),
            scenario,
            recent_messages: vec![],
        }));
        let raw = r#"{
            "player_response": "The examiner nods cautiously.",
            "world_state_delta": {
                "facts_to_add": [],
                "npc_changes": [],
                "faction_changes": [{"type":"standing_changed","faction_id":"guild","standing_delta":-3,"reason":"Suspicious behavior."}],
                "quest_changes": [],
                "clock_changes": [],
                "relationship_changes": [],
                "location_change": null,
                "event_log_entries": []
            }
        }"#;
        let provider = Arc::new(MockProvider::new("mock", [raw.into()]));
        let pipeline = DefaultTurnPipeline::new(provider, Arc::clone(&store));

        pipeline
            .process_turn_debug(TurnRequestInput {
                session_id,
                input: "I act suspiciously.".into(),
                mode: Some(TurnMode::Action),
                viewer: ViewerContext::player(),
            })
            .await
            .unwrap();

        // World state must be persisted with version 1
        let persisted = store
            .persisted_state
            .lock()
            .expect("state mutex")
            .clone()
            .expect("persisted state");
        assert_eq!(persisted.version, 1);

        // Both user and assistant messages must have been persisted
        let messages = store
            .persisted_messages
            .lock()
            .expect("messages mutex")
            .clone();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages[1].role, MessageRole::Assistant);
    }
}
