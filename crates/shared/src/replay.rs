use domain::{
    FactVisibility, FrontendVisibleState, Scenario, TurnMode, WorldState, WorldStateDelta,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn default_replay_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayFixture {
    #[serde(default = "default_replay_version")]
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub source_session_id: Option<Uuid>,
    pub scenario: Scenario,
    #[serde(default)]
    pub turns: Vec<ReplayTurn>,
    pub expected_final: ExpectedFinalState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayTurn {
    pub input: String,
    #[serde(default)]
    pub mode: Option<TurnMode>,
    pub provider_response: serde_json::Value,
    #[serde(default)]
    pub expected_response_contains: Vec<String>,
    #[serde(default)]
    pub expected_delta: Option<WorldStateDelta>,
    #[serde(default)]
    pub expected_status: Option<u16>,
}

impl ReplayTurn {
    pub fn expected_status_code(&self) -> u16 {
        self.expected_status.unwrap_or(200)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExpectedFinalState {
    pub world_state_version: i64,
    #[serde(default)]
    pub visible_fact_contains: Vec<String>,
    #[serde(default)]
    pub visible_memory_contains: Vec<String>,
    #[serde(default)]
    pub hidden_fact_ids_absent_from_projection: Vec<String>,
}

pub fn build_replay_fixture_draft(
    name: String,
    source_session_id: Option<Uuid>,
    scenario: Scenario,
    world_state: &WorldState,
    visible_state: &FrontendVisibleState,
) -> ReplayFixture {
    ReplayFixture {
        version: default_replay_version(),
        name,
        source_session_id,
        scenario,
        turns: vec![],
        expected_final: ExpectedFinalState {
            world_state_version: visible_state.state_version,
            visible_fact_contains: visible_state
                .player_known_facts
                .iter()
                .map(|fact| fact.text.clone())
                .collect(),
            visible_memory_contains: visible_state
                .visible_memories
                .iter()
                .map(|memory| memory.text.clone())
                .collect(),
            hidden_fact_ids_absent_from_projection: world_state
                .facts
                .iter()
                .filter(|fact| fact.visibility == FactVisibility::GmOnly)
                .map(|fact| fact.id.clone())
                .collect(),
        },
    }
}
