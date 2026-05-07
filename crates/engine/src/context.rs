use domain::{
    Fact, FactVisibility, Faction, FactionState, Location, Npc, NpcStatus, QuestState, Scenario,
    SceneReasoningStyle, TurnMode, WorldState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleActivationContext {
    pub active_role_name: Option<String>,
    pub emotion_now: Option<String>,
    pub motivation_now: Option<String>,
    pub knowledge_boundaries: Vec<String>,
    pub forbidden_moves: Vec<String>,
    pub speech_constraints: Vec<String>,
}

pub trait RoleIdentityActivator: Send + Sync {
    fn activate(
        &self,
        scenario: &Scenario,
        world_state: &WorldState,
        scene_style: SceneReasoningStyle,
    ) -> RoleActivationContext;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicRoleIdentityActivator;

impl RoleIdentityActivator for BasicRoleIdentityActivator {
    fn activate(
        &self,
        scenario: &Scenario,
        world_state: &WorldState,
        _scene_style: SceneReasoningStyle,
    ) -> RoleActivationContext {
        let active_npc = world_state
            .active_speaker_id
            .as_ref()
            .and_then(|id| scenario.npcs.iter().find(|npc| &npc.id == id));

        if let Some(npc) = active_npc {
            let runtime_status = world_state
                .npcs
                .iter()
                .find(|state| state.npc_id == npc.id)
                .map(|state| state.status)
                .unwrap_or(npc.initial_status);

            if runtime_status.can_act() {
                return RoleActivationContext {
                    active_role_name: Some(npc.name.clone()),
                    emotion_now: Some(npc.role_identity.core_emotion.clone()),
                    motivation_now: Some(npc.role_identity.motivation.clone()),
                    knowledge_boundaries: npc.role_identity.boundaries.clone(),
                    forbidden_moves: vec![
                        "do not reveal GM-only secrets without justified discovery".into(),
                        "do not use knowledge this role does not have".into(),
                    ],
                    speech_constraints: vec![npc.role_identity.speech_style.clone()],
                };
            }
        }

        RoleActivationContext {
            active_role_name: None,
            emotion_now: None,
            motivation_now: Some(
                "adjudicate consequences while preserving world consistency".into(),
            ),
            knowledge_boundaries: vec![
                "GM-only facts may shape foreshadowing but must not be revealed early".into(),
            ],
            forbidden_moves: vec![
                "do not directly overwrite world state".into(),
                "do not expose hidden reasoning".into(),
            ],
            speech_constraints: vec!["immersive storyteller narration".into()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningStyleDirective {
    pub style: SceneReasoningStyle,
    pub priorities: Vec<String>,
    pub avoid: Vec<String>,
    pub visible_response_shape: String,
}

pub trait ReasoningStyleOptimizer: Send + Sync {
    fn directive_for(&self, style: SceneReasoningStyle) -> ReasoningStyleDirective;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicReasoningStyleOptimizer;

impl ReasoningStyleOptimizer for BasicReasoningStyleOptimizer {
    fn directive_for(&self, style: SceneReasoningStyle) -> ReasoningStyleDirective {
        let (priorities, avoid, shape): (&[&str], &[&str], &str) = match style {
            SceneReasoningStyle::PoliticalNegotiation => (
                &[
                    "track leverage",
                    "preserve faction interests",
                    "show public and private consequences",
                ],
                &[
                    "instant loyalty change",
                    "generic exposition",
                    "ignoring reputation",
                ],
                "immersive dialogue plus visible social consequence",
            ),
            SceneReasoningStyle::TacticalCombat => (
                &[
                    "clear action resolution",
                    "enemy intent",
                    "stakes beyond player HP",
                ],
                &[
                    "vague cinematic fog",
                    "denying established powers",
                    "one power solving every stake",
                ],
                "crisp action resolution with external stakes",
            ),
            SceneReasoningStyle::MysteryInvestigation => (
                &[
                    "clue discipline",
                    "partial reveals",
                    "player-known vs GM-only facts",
                ],
                &[
                    "full mystery reveal",
                    "answers without discovery",
                    "contradicting established clues",
                ],
                "evidence-focused discovery response",
            ),
            SceneReasoningStyle::RulesAdjudication => (
                &["concise ruling", "fairness", "clear options"],
                &[
                    "long lore monologues",
                    "arbitrary hidden restrictions",
                    "mid-scene rule changes",
                ],
                "clear ruling with consequences",
            ),
            _ => (
                &[
                    "NPC motivation",
                    "relationship memory",
                    "speech style",
                    "what the NPC knows",
                ],
                &[
                    "generic assistant advice",
                    "exposition dumps",
                    "revealing secrets too early",
                ],
                "immersive dialogue or narration",
            ),
        };

        ReasoningStyleDirective {
            style,
            priorities: priorities.iter().map(|value| (*value).to_owned()).collect(),
            avoid: avoid.iter().map(|value| (*value).to_owned()).collect(),
            visible_response_shape: shape.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentContext {
    pub scenario_title: String,
    pub setting_summary: String,
    pub current_location: Option<Location>,
    pub active_role: RoleActivationContext,
    pub scene_directive: ReasoningStyleDirective,
    pub relevant_npcs: Vec<NpcContext>,
    pub relevant_factions: Vec<FactionContext>,
    pub active_quests: Vec<QuestState>,
    pub active_clocks: Vec<domain::ClockState>,
    pub player_known_facts: Vec<Fact>,
    pub gm_only_facts: Vec<Fact>,
    pub recent_summary: Option<String>,
    pub recent_messages: Vec<MessageContext>,
    pub rules: Vec<String>,
    pub mode: Option<TurnMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcContext {
    pub npc: Npc,
    pub status: NpcStatus,
    pub attitude_to_player: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactionContext {
    pub faction: Faction,
    pub state: Option<FactionState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageContext {
    pub role: String,
    pub content: String,
}

pub struct BuildContextInput<'a> {
    pub scenario: &'a Scenario,
    pub world_state: &'a WorldState,
    pub active_role: RoleActivationContext,
    pub scene_directive: ReasoningStyleDirective,
    pub recent_messages: Vec<MessageContext>,
    pub mode: Option<TurnMode>,
}

pub trait ContextBuilder: Send + Sync {
    fn build(&self, input: BuildContextInput<'_>) -> AgentContext;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicContextBuilder;

impl ContextBuilder for BasicContextBuilder {
    fn build(&self, input: BuildContextInput<'_>) -> AgentContext {
        let current_location = input
            .world_state
            .current_location_id
            .as_ref()
            .and_then(|id| {
                input
                    .scenario
                    .locations
                    .iter()
                    .find(|location| &location.id == id)
            })
            .cloned();

        let relevant_npcs = input
            .scenario
            .npcs
            .iter()
            .filter(|npc| {
                input.world_state.active_speaker_id.as_ref() == Some(&npc.id)
                    || input.world_state.npcs.iter().any(|state| {
                        state.npc_id == npc.id
                            && state.location_id == input.world_state.current_location_id
                    })
            })
            .map(|npc| {
                let runtime = input
                    .world_state
                    .npcs
                    .iter()
                    .find(|state| state.npc_id == npc.id);
                NpcContext {
                    npc: npc.clone(),
                    status: runtime
                        .map(|state| state.status)
                        .unwrap_or(npc.initial_status),
                    attitude_to_player: runtime.and_then(|state| state.attitude_to_player.clone()),
                }
            })
            .collect();

        let relevant_factions = input
            .scenario
            .factions
            .iter()
            .filter_map(|faction| {
                let state = input
                    .world_state
                    .factions
                    .iter()
                    .find(|state| state.faction_id == faction.id)
                    .cloned();
                state.map(|state| FactionContext {
                    faction: faction.clone(),
                    state: Some(state),
                })
            })
            .collect();

        let player_known_facts = input
            .world_state
            .facts
            .iter()
            .filter(|fact| fact.visibility == FactVisibility::PlayerKnown)
            .cloned()
            .collect();
        let gm_only_facts = input
            .world_state
            .facts
            .iter()
            .filter(|fact| fact.visibility == FactVisibility::GmOnly)
            .take(5)
            .cloned()
            .collect();

        AgentContext {
            scenario_title: input.scenario.title.clone(),
            setting_summary: input.scenario.setting.clone(),
            current_location,
            active_role: input.active_role,
            scene_directive: input.scene_directive,
            relevant_npcs,
            relevant_factions,
            active_quests: input
                .world_state
                .quests
                .iter()
                .filter(|quest| {
                    matches!(
                        quest.status,
                        domain::QuestStatus::Active | domain::QuestStatus::Available
                    )
                })
                .cloned()
                .collect(),
            active_clocks: input.world_state.clocks.clone(),
            player_known_facts,
            gm_only_facts,
            recent_summary: input.world_state.summary.clone(),
            recent_messages: input
                .recent_messages
                .into_iter()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            rules: input.scenario.rules.clone(),
            mode: input.mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::*;
    use uuid::Uuid;

    fn scenario() -> Scenario {
        Scenario {
            id: Uuid::new_v4(),
            title: "Aurethia".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "high fantasy".into(),
            tone: "heroic".into(),
            rules: vec![],
            locations: vec![Location {
                id: "hall".into(),
                name: "Hall".into(),
                description: "Marble hall.".into(),
                visible: true,
            }],
            factions: vec![],
            npcs: vec![Npc {
                id: "seraphyne".into(),
                name: "Seraphyne".into(),
                description: "Goddess.".into(),
                role_identity: RoleIdentity {
                    core_emotion: "worried".into(),
                    motivation: "guide carefully".into(),
                    worldview: "power requires responsibility".into(),
                    fear: None,
                    desire: None,
                    speech_style: "solemn".into(),
                    boundaries: vec!["does not know the void source".into()],
                    values: vec![],
                },
                stats: None,
                initial_status: NpcStatus::Active,
            }],
            quests: vec![],
            secrets: vec![],
            clocks: vec![],
        }
    }

    fn state(status: NpcStatus) -> WorldState {
        WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 0,
            current_location_id: Some("hall".into()),
            current_scene: None,
            active_speaker_id: Some("seraphyne".into()),
            facts: vec![],
            npcs: vec![NpcState {
                npc_id: "seraphyne".into(),
                status,
                visible_to_player: true,
                location_id: Some("hall".into()),
                attitude_to_player: Some("cautious warmth".into()),
                known_facts: vec![],
                notes: vec![],
            }],
            factions: vec![],
            quests: vec![],
            clocks: vec![],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        }
    }

    #[test]
    fn active_npc_role_is_activated_when_able_to_act() {
        let role = BasicRoleIdentityActivator.activate(
            &scenario(),
            &state(NpcStatus::Active),
            SceneReasoningStyle::CharacterDialogue,
        );

        assert_eq!(role.active_role_name.as_deref(), Some("Seraphyne"));
        assert!(role.knowledge_boundaries[0].contains("void source"));
    }

    #[test]
    fn unconscious_npc_falls_back_to_narrator_mode() {
        let role = BasicRoleIdentityActivator.activate(
            &scenario(),
            &state(NpcStatus::Unconscious),
            SceneReasoningStyle::CharacterDialogue,
        );

        assert_eq!(role.active_role_name, None);
        assert!(
            role.forbidden_moves
                .iter()
                .any(|item| item.contains("world state"))
        );
    }
}
