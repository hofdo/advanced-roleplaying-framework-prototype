use crate::{EntityKey, MessageId, ScenarioId, SessionId};
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
    pub relationships: Vec<RelationshipState>,
    pub inventory: Vec<InventoryItem>,
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
    pub reveal_conditions: Vec<String>,
    #[serde(default)]
    pub related_secret_ids: Vec<EntityKey>,
    #[serde(default)]
    pub reveal_condition_satisfied: Option<String>,
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
}

fn default_visible_to_player() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactionState {
    pub faction_id: EntityKey,
    pub standing: i32,
    pub public_notes: Vec<String>,
    pub hidden_notes: Vec<String>,
    pub revealed_goals: Vec<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipState {
    pub source_id: EntityKey,
    pub target_id: EntityKey,
    pub attitude: i32,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventoryItem {
    pub id: EntityKey,
    pub name: String,
    pub description: String,
    pub visible: bool,
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
    pub facts_to_add: Vec<FactToAdd>,
    pub npc_changes: Vec<NpcChange>,
    pub faction_changes: Vec<FactionChange>,
    pub quest_changes: Vec<QuestChange>,
    pub clock_changes: Vec<ClockChange>,
    pub relationship_changes: Vec<RelationshipChange>,
    pub location_change: Option<LocationChange>,
    pub event_log_entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactToAdd {
    pub text: String,
    pub visibility: FactVisibility,
    pub known_by: Vec<EntityKey>,
    pub reveal_conditions: Vec<String>,
    pub reason: String,
    #[serde(default)]
    pub related_secret_ids: Vec<EntityKey>,
    #[serde(default)]
    pub reveal_condition_satisfied: Option<String>,
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
    pub visible_quests: Vec<VisibleQuest>,
    pub visible_clocks: Vec<VisibleClock>,
    pub player_known_facts: Vec<VisibleFact>,
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
