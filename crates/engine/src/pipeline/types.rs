use crate::{AgentContext, ValidatedWorldStateDelta};
use domain::{
    EntityRef, FrontendStatePatch, MessageRecord, Scenario, SceneReasoningStyle, SessionId,
    TurnMode, ViewerContext, WorldState, WorldStateDelta,
};
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

/// Holds everything needed to start a streaming (or non-streaming) provider call.
/// Produced by [`crate::DefaultTurnPipeline::prepare_turn_context`].
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
/// Produced by [`crate::DefaultTurnPipeline::finalize_turn_delta`].
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
