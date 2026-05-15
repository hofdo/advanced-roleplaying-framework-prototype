use crate::{EntityKey, ScenarioId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scenario {
    pub id: ScenarioId,
    pub title: String,
    pub scenario_type: ScenarioType,
    pub setting: String,
    pub tone: String,
    pub rules: Vec<String>,
    pub locations: Vec<Location>,
    pub factions: Vec<Faction>,
    pub npcs: Vec<Npc>,
    pub quests: Vec<Quest>,
    pub secrets: Vec<Secret>,
    pub clocks: Vec<ClockTemplate>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioType {
    Adventure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Location {
    pub id: EntityKey,
    pub name: String,
    pub description: String,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleIdentity {
    pub core_emotion: String,
    pub motivation: String,
    pub worldview: String,
    pub fear: Option<String>,
    pub desire: Option<String>,
    pub speech_style: String,
    pub boundaries: Vec<String>,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CharacterStats {
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Npc {
    pub id: EntityKey,
    pub name: String,
    pub description: String,
    pub role_identity: RoleIdentity,
    pub stats: Option<CharacterStats>,
    pub initial_status: NpcStatus,
    #[serde(default)]
    pub initial_location_id: Option<EntityKey>,
    #[serde(default = "default_initial_visible_to_player")]
    pub initial_visible_to_player: bool,
}

fn default_initial_visible_to_player() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NpcStatus {
    Active,
    Injured,
    Unconscious,
    Missing,
    Captured,
    Dead,
    Hidden,
    Unknown,
}

impl NpcStatus {
    pub fn can_act(self) -> bool {
        matches!(self, Self::Active | Self::Injured | Self::Unknown)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactionIdentity {
    pub public_goal: String,
    pub hidden_goal: Option<String>,
    pub values: Vec<String>,
    pub fears: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Faction {
    pub id: EntityKey,
    pub name: String,
    pub description: String,
    pub faction_identity: FactionIdentity,
    pub initial_standing: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Quest {
    pub id: EntityKey,
    pub title: String,
    pub description: String,
    pub objectives: Vec<QuestObjective>,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestObjective {
    pub id: EntityKey,
    pub title: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Secret {
    pub id: EntityKey,
    pub text: String,
    #[serde(default)]
    pub reveal_conditions: Vec<RevealCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevealCondition {
    pub id: EntityKey,
    pub description: String,
}

impl From<&str> for RevealCondition {
    fn from(value: &str) -> Self {
        Self {
            id: value.into(),
            description: value.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClockTemplate {
    pub id: EntityKey,
    pub title: String,
    pub current: u8,
    pub max: u8,
    pub consequence: String,
}
