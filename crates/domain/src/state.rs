use crate::{EntityKey, MessageId, RevealCondition, ScenarioId, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorldState {
    pub session_id: SessionId,
    pub scenario_id: ScenarioId,
    pub version: i64,
    pub current_location_id: Option<EntityKey>,
    pub current_scene: Option<String>,
    pub active_speaker_id: Option<EntityKey>,
    pub facts: Vec<Fact>,
    pub npcs: Vec<NpcState>,
    pub factions: Vec<FactionState>,
    pub quests: Vec<QuestState>,
    pub clocks: Vec<ClockState>,
    #[serde(default)]
    pub action_resolutions: Vec<ActionResolution>,
    pub relationships: Vec<RelationshipState>,
    pub inventory: Vec<InventoryItem>,
    #[serde(default)]
    pub player: PlayerCharacterState,
    #[serde(default)]
    pub clues: Vec<ClueState>,
    #[serde(default)]
    pub memories: Vec<MemoryEntry>,
    pub summary: Option<String>,
    pub recent_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Fact {
    pub id: EntityKey,
    pub text: String,
    pub visibility: FactVisibility,
    pub known_by: Vec<EntityKey>,
    pub source: FactSource,
    #[serde(default)]
    pub reveal_conditions: Vec<RevealCondition>,
    #[serde(default)]
    pub related_secret_ids: Vec<EntityKey>,
    #[serde(default)]
    pub reveal_condition_satisfied: Option<ConditionRef>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactVisibility {
    PlayerKnown,
    GmOnly,
    NpcKnown,
    FactionKnown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactSource {
    Scenario,
    Turn,
    PlayerCorrection,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NpcState {
    pub npc_id: EntityKey,
    pub status: crate::NpcStatus,
    #[serde(default = "default_visible_to_player")]
    pub visible_to_player: bool,
    pub location_id: Option<EntityKey>,
    pub attitude_to_player: Option<String>,
    pub known_facts: Vec<EntityKey>,
    pub notes: Vec<String>,
    #[serde(default = "default_npc_availability")]
    pub availability: NpcAvailability,
    #[serde(default)]
    pub current_intent: Option<String>,
    #[serde(default)]
    pub offscreen_actions: Vec<OffscreenAction>,
}

fn default_visible_to_player() -> bool {
    true
}

fn default_npc_availability() -> NpcAvailability {
    NpcAvailability::Present
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NpcAvailability {
    Present,
    Nearby,
    Offscreen,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OffscreenAction {
    pub intent: String,
    pub result: String,
    pub visible_to_player: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactionState {
    pub faction_id: EntityKey,
    pub standing: i32,
    pub public_notes: Vec<String>,
    pub hidden_notes: Vec<String>,
    pub revealed_goals: Vec<String>,
    #[serde(default)]
    pub pressure: i32,
    #[serde(default)]
    pub public_pressure_notes: Vec<String>,
    #[serde(default)]
    pub hidden_pressure_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestState {
    pub quest_id: EntityKey,
    pub status: QuestStatus,
    pub completed_objectives: Vec<EntityKey>,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuestStatus {
    Available,
    Active,
    Completed,
    Failed,
    Hidden,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClockState {
    pub id: EntityKey,
    pub title: String,
    pub current: u8,
    pub max: u8,
    pub consequence: String,
    #[serde(default = "default_visible_to_player")]
    pub visible_to_player: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipState {
    pub source_id: EntityKey,
    pub target_id: EntityKey,
    pub attitude: i32,
    pub notes: Vec<String>,
    #[serde(default)]
    pub trust: i32,
    #[serde(default)]
    pub suspicion: i32,
    #[serde(default)]
    pub loyalty: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionResolution {
    pub id: EntityKey,
    pub intent: String,
    pub stakes: Vec<String>,
    pub outcome: ActionOutcome,
    pub consequence: String,
    pub visible_to_player: bool,
    pub linked_clock_ids: Vec<EntityKey>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    Success,
    SuccessWithCost,
    Partial,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PlayerCharacterState {
    #[serde(default)]
    pub traits: Vec<PlayerTrait>,
    #[serde(default)]
    pub goals: Vec<PlayerGoal>,
    #[serde(default)]
    pub conditions: Vec<PlayerCondition>,
    #[serde(default)]
    pub resources: Vec<PlayerResource>,
    #[serde(default)]
    pub gm_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerTrait {
    pub id: EntityKey,
    pub label: String,
    pub description: String,
    #[serde(default = "default_visible_to_player")]
    pub visible_to_player: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerGoal {
    pub id: EntityKey,
    pub label: String,
    pub description: String,
    pub progress: i32,
    #[serde(default = "default_visible_to_player")]
    pub visible_to_player: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerCondition {
    pub id: EntityKey,
    pub label: String,
    pub description: String,
    #[serde(default = "default_visible_to_player")]
    pub visible_to_player: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerResource {
    pub id: EntityKey,
    pub label: String,
    pub current: i32,
    pub min: i32,
    pub max: i32,
    #[serde(default = "default_visible_to_player")]
    pub visible_to_player: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConditionRef {
    pub id: EntityKey,
    pub mode: MatchMode,
}

impl From<&str> for ConditionRef {
    fn from(value: &str) -> Self {
        Self {
            id: value.into(),
            mode: MatchMode::Exact,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    Exact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClueState {
    pub id: EntityKey,
    pub text: String,
    #[serde(default)]
    pub linked_secret_ids: Vec<EntityKey>,
    #[serde(default)]
    pub satisfied_reveal_conditions: Vec<ConditionRef>,
    #[serde(default = "default_visible_to_player")]
    pub visible_to_player: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventoryItem {
    pub id: EntityKey,
    pub name: String,
    pub description: String,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryEntry {
    pub id: EntityKey,
    pub text: String,
    pub visibility: MemoryVisibility,
    pub importance: u8,
    pub related_entity_ids: Vec<EntityKey>,
    pub source_message_id: Option<MessageId>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVisibility {
    PlayerKnown,
    GmOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SceneReasoningStyle {
    CharacterDialogue,
    EmotionalScene,
    PoliticalNegotiation,
    MysteryInvestigation,
    TacticalCombat,
    WorldSimulation,
    RulesAdjudication,
    TravelExploration,
    Downtime,
    QuestResolution,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnMode {
    Dialogue,
    Action,
    Direct,
    Remember,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorldStateDelta {
    #[serde(default)]
    pub scene_change: Option<SceneChange>,
    #[serde(default)]
    pub active_speaker_change: Option<ActiveSpeakerChange>,
    #[serde(default)]
    pub facts_to_add: Vec<FactToAdd>,
    #[serde(default)]
    pub action_resolution_changes: Vec<ActionResolutionChange>,
    #[serde(default)]
    pub npc_changes: Vec<NpcChange>,
    #[serde(default)]
    pub faction_changes: Vec<FactionChange>,
    #[serde(default)]
    pub quest_changes: Vec<QuestChange>,
    #[serde(default)]
    pub clock_changes: Vec<ClockChange>,
    #[serde(default)]
    pub relationship_changes: Vec<RelationshipChange>,
    #[serde(default)]
    pub inventory_changes: Vec<InventoryChange>,
    #[serde(default)]
    pub player_changes: Vec<PlayerChange>,
    #[serde(default)]
    pub clue_changes: Vec<ClueChange>,
    #[serde(default)]
    pub location_change: Option<LocationChange>,
    #[serde(default)]
    pub summary_update: Option<SummaryUpdate>,
    #[serde(default)]
    pub memory_changes: Vec<MemoryChange>,
    #[serde(default)]
    pub event_log_entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemoryChange {
    Added {
        text: String,
        visibility: MemoryVisibility,
        importance: u8,
        related_entity_ids: Vec<EntityKey>,
        reason: String,
    },
    ImportanceChanged {
        memory_id: EntityKey,
        importance: u8,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SceneChange {
    pub scene: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveSpeakerChange {
    pub speaker_id: Option<EntityKey>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SummaryUpdate {
    pub summary: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactToAdd {
    pub text: String,
    pub visibility: FactVisibility,
    pub known_by: Vec<EntityKey>,
    #[serde(default)]
    pub reveal_conditions: Vec<RevealCondition>,
    pub reason: String,
    #[serde(default)]
    pub related_secret_ids: Vec<EntityKey>,
    #[serde(default)]
    pub reveal_condition_satisfied: Option<ConditionRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionResolutionChange {
    Recorded {
        intent: String,
        stakes: Vec<String>,
        outcome: ActionOutcome,
        consequence: String,
        visible_to_player: bool,
        linked_clock_ids: Vec<EntityKey>,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NpcChange {
    AttitudeChanged {
        npc_id: EntityKey,
        attitude: String,
        reason: String,
    },
    KnowledgeAdded {
        npc_id: EntityKey,
        fact: String,
        visibility: FactVisibility,
        reason: String,
    },
    StatusChanged {
        npc_id: EntityKey,
        status: crate::NpcStatus,
        reason: String,
    },
    LocationChanged {
        npc_id: EntityKey,
        location_id: EntityKey,
        reason: String,
    },
    NoteAdded {
        npc_id: EntityKey,
        note: String,
        reason: String,
    },
    VisibilityChanged {
        npc_id: EntityKey,
        visible_to_player: bool,
        reason: String,
    },
    AvailabilityChanged {
        npc_id: EntityKey,
        availability: NpcAvailability,
        reason: String,
    },
    IntentChanged {
        npc_id: EntityKey,
        intent: Option<String>,
        reason: String,
    },
    OffscreenActionRecorded {
        npc_id: EntityKey,
        intent: String,
        result: String,
        visible_to_player: bool,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FactionChange {
    StandingChanged {
        faction_id: EntityKey,
        standing_delta: i32,
        reason: String,
    },
    GoalRevealed {
        faction_id: EntityKey,
        goal: String,
        reason: String,
    },
    PublicNoteAdded {
        faction_id: EntityKey,
        note: String,
        reason: String,
    },
    HiddenNoteAdded {
        faction_id: EntityKey,
        note: String,
        reason: String,
    },
    PressureChanged {
        faction_id: EntityKey,
        delta: i32,
        public: bool,
        reason: String,
    },
    PublicPressureNoteAdded {
        faction_id: EntityKey,
        note: String,
        reason: String,
    },
    HiddenPressureNoteAdded {
        faction_id: EntityKey,
        note: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClockChange {
    Advanced {
        clock_id: EntityKey,
        delta: i8,
        reason: String,
    },
    SetValue {
        clock_id: EntityKey,
        value: u8,
        reason: String,
    },
    VisibilityChanged {
        clock_id: EntityKey,
        visible_to_player: bool,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuestChange {
    Started {
        quest_id: EntityKey,
        reason: String,
    },
    ObjectiveCompleted {
        quest_id: EntityKey,
        objective_id: EntityKey,
        reason: String,
    },
    Completed {
        quest_id: EntityKey,
        reason: String,
    },
    Failed {
        quest_id: EntityKey,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelationshipChange {
    Changed {
        source_id: EntityKey,
        target_id: EntityKey,
        attitude_delta: i32,
        reason: String,
    },
    NoteAdded {
        source_id: EntityKey,
        target_id: EntityKey,
        note: String,
        reason: String,
    },
    TrustChanged {
        source_id: EntityKey,
        target_id: EntityKey,
        delta: i32,
        reason: String,
    },
    SuspicionChanged {
        source_id: EntityKey,
        target_id: EntityKey,
        delta: i32,
        reason: String,
    },
    LoyaltyChanged {
        source_id: EntityKey,
        target_id: EntityKey,
        delta: i32,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InventoryChange {
    Added { item: InventoryItem, reason: String },
    Removed { item_id: EntityKey, reason: String },
    Updated { item: InventoryItem, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerChange {
    TraitAdded {
        trait_id: EntityKey,
        label: String,
        description: String,
        visible_to_player: bool,
        reason: String,
    },
    GoalAdded {
        goal_id: EntityKey,
        label: String,
        description: String,
        progress: i32,
        visible_to_player: bool,
        reason: String,
    },
    GoalProgressed {
        goal_id: EntityKey,
        delta: i32,
        reason: String,
    },
    ConditionAdded {
        condition_id: EntityKey,
        label: String,
        description: String,
        visible_to_player: bool,
        reason: String,
    },
    ConditionCleared {
        condition_id: EntityKey,
        reason: String,
    },
    ResourceChanged {
        resource_id: EntityKey,
        delta: i32,
        reason: String,
    },
    GmNoteAdded {
        note: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClueChange {
    Discovered {
        clue_id: EntityKey,
        text: String,
        linked_secret_ids: Vec<EntityKey>,
        satisfied_reveal_conditions: Vec<ConditionRef>,
        visible_to_player: bool,
        reason: String,
    },
    VisibilityChanged {
        clue_id: EntityKey,
        visible_to_player: bool,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocationChange {
    pub location_id: EntityKey,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrontendVisibleState {
    pub state_version: i64,
    pub current_location: Option<VisibleLocation>,
    pub active_speaker: Option<VisibleNpc>,
    pub visible_npcs: Vec<VisibleNpc>,
    #[serde(default)]
    pub visible_factions: Vec<VisibleFaction>,
    #[serde(default)]
    pub visible_relationships: Vec<VisibleRelationship>,
    pub visible_quests: Vec<VisibleQuest>,
    pub visible_clocks: Vec<VisibleClock>,
    pub player_known_facts: Vec<VisibleFact>,
    #[serde(default)]
    pub visible_action_resolutions: Vec<VisibleActionResolution>,
    #[serde(default)]
    pub visible_clues: Vec<VisibleClue>,
    #[serde(default)]
    pub player: VisiblePlayerCharacterState,
    #[serde(default)]
    pub visible_memories: Vec<VisibleMemory>,
    pub recent_public_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrontendStatePatch {
    pub state_version: i64,
    pub changed_entities: Vec<EntityRef>,
    pub visible_state: Option<FrontendVisibleState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityRef {
    pub entity_type: String,
    pub id: EntityKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ViewerContext {
    pub include_debug_state: bool,
    pub is_admin: bool,
}

impl ViewerContext {
    pub fn player() -> Self {
        Self {
            include_debug_state: false,
            is_admin: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleLocation {
    pub id: EntityKey,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleNpc {
    pub id: EntityKey,
    pub name: String,
    pub description: String,
    pub status: crate::NpcStatus,
    pub attitude_to_player: Option<String>,
    pub availability: NpcAvailability,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleFaction {
    pub id: EntityKey,
    pub name: String,
    pub standing: i32,
    pub pressure: i32,
    pub public_notes: Vec<String>,
    pub public_pressure_notes: Vec<String>,
    pub revealed_goals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleRelationship {
    pub source_id: EntityKey,
    pub target_id: EntityKey,
    pub attitude: i32,
    pub trust: i32,
    pub suspicion: i32,
    pub loyalty: i32,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleQuest {
    pub id: EntityKey,
    pub status: QuestStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleClock {
    pub id: EntityKey,
    pub title: String,
    pub current: u8,
    pub max: u8,
    pub consequence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleFact {
    pub id: EntityKey,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleActionResolution {
    pub id: EntityKey,
    pub intent: String,
    pub stakes: Vec<String>,
    pub outcome: ActionOutcome,
    pub consequence: String,
    pub linked_clock_ids: Vec<EntityKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleClue {
    pub id: EntityKey,
    pub text: String,
    pub satisfied_reveal_conditions: Vec<ConditionRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VisiblePlayerCharacterState {
    #[serde(default)]
    pub traits: Vec<PlayerTrait>,
    #[serde(default)]
    pub goals: Vec<PlayerGoal>,
    #[serde(default)]
    pub conditions: Vec<PlayerCondition>,
    #[serde(default)]
    pub resources: Vec<PlayerResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisibleMemory {
    pub id: EntityKey,
    pub text: String,
    pub importance: u8,
    pub related_entity_ids: Vec<EntityKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageRecord {
    pub id: MessageId,
    pub session_id: SessionId,
    pub role: MessageRole,
    pub speaker_id: Option<EntityKey>,
    pub content: String,
    pub scene_type: Option<SceneReasoningStyle>,
    pub prompt_template_version: Option<String>,
    pub raw_provider_output: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}
