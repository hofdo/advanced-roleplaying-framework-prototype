use domain::{
    Fact, FactVisibility, Faction, FactionState, Location, MemoryEntry, MemoryVisibility, Npc,
    NpcStatus, QuestState, Scenario, SceneReasoningStyle, TurnMode, WorldState,
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
    pub player_memories: Vec<MemoryEntry>,
    pub gm_only_memories: Vec<MemoryEntry>,
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
        let player_memories = prioritized_memories(
            &input.world_state.memories,
            MemoryVisibility::PlayerKnown,
            8,
        );
        let gm_only_memories =
            prioritized_memories(&input.world_state.memories, MemoryVisibility::GmOnly, 5);

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
            player_memories,
            gm_only_memories,
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

fn prioritized_memories(
    memories: &[MemoryEntry],
    visibility: MemoryVisibility,
    limit: usize,
) -> Vec<MemoryEntry> {
    let mut indexed = memories
        .iter()
        .enumerate()
        .filter(|(_, memory)| memory.visibility == visibility)
        .collect::<Vec<_>>();
    indexed.sort_by(|(left_idx, left), (right_idx, right)| {
        right
            .importance
            .cmp(&left.importance)
            .then(left_idx.cmp(right_idx))
    });
    indexed
        .into_iter()
        .take(limit)
        .map(|(_, memory)| memory.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::fixtures;
    use domain::{
        MemoryEntry, MemoryVisibility, NpcStatus, Scenario, SceneReasoningStyle, WorldState,
    };

    fn scenario() -> Scenario {
        let mut scenario = fixtures::scenario()
            .with_location("hall", "Hall")
            .with_npc("seraphyne", "Seraphyne")
            .build();
        scenario.locations.retain(|location| location.id == "hall");
        scenario.npcs.retain(|npc| npc.id == "seraphyne");
        let seraphyne = scenario
            .npcs
            .iter_mut()
            .find(|npc| npc.id == "seraphyne")
            .expect("seraphyne fixture");
        seraphyne.role_identity.core_emotion = "worried".into();
        seraphyne.role_identity.motivation = "guide carefully".into();
        seraphyne.role_identity.worldview = "power requires responsibility".into();
        seraphyne.role_identity.speech_style = "solemn".into();
        seraphyne.role_identity.boundaries = vec!["does not know the void source".into()];
        seraphyne.description = "Goddess.".into();
        seraphyne.initial_location_id = Some("hall".into());
        scenario.factions.clear();
        scenario.quests.clear();
        scenario.secrets.clear();
        scenario.clocks.clear();
        scenario
    }

    fn state(status: NpcStatus) -> WorldState {
        let scenario = scenario();
        let mut world = fixtures::world_state(&scenario).build();
        world.npcs[0].status = status;
        world.npcs[0].attitude_to_player = Some("cautious warmth".into());
        world.factions.clear();
        world.quests.clear();
        world.clocks.clear();
        world.facts.clear();
        world
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

    #[test]
    fn build_context_selects_memories_by_visibility_and_importance() {
        let scenario = scenario();
        let mut world = state(NpcStatus::Active);
        world.memories = vec![
            MemoryEntry {
                id: "p1".into(),
                text: "low".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 1,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "p2".into(),
                text: "high-a".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 9,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "p3".into(),
                text: "high-b".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 9,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "p4".into(),
                text: "mid".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 4,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "p5".into(),
                text: "upper".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 7,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "p6".into(),
                text: "lower".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 3,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "p7".into(),
                text: "upper-mid".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 6,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "p8".into(),
                text: "tiny".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 2,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "p9".into(),
                text: "very-high".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 8,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "g1".into(),
                text: "gm-low".into(),
                visibility: MemoryVisibility::GmOnly,
                importance: 2,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "g2".into(),
                text: "gm-high-a".into(),
                visibility: MemoryVisibility::GmOnly,
                importance: 10,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "g3".into(),
                text: "gm-high-b".into(),
                visibility: MemoryVisibility::GmOnly,
                importance: 10,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "g4".into(),
                text: "gm-mid".into(),
                visibility: MemoryVisibility::GmOnly,
                importance: 5,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "g5".into(),
                text: "gm-upper".into(),
                visibility: MemoryVisibility::GmOnly,
                importance: 7,
                related_entity_ids: vec![],
                source_message_id: None,
            },
            MemoryEntry {
                id: "g6".into(),
                text: "gm-tiny".into(),
                visibility: MemoryVisibility::GmOnly,
                importance: 1,
                related_entity_ids: vec![],
                source_message_id: None,
            },
        ];

        let context = BasicContextBuilder.build(BuildContextInput {
            scenario: &scenario,
            world_state: &world,
            active_role: BasicRoleIdentityActivator.activate(
                &scenario,
                &world,
                SceneReasoningStyle::CharacterDialogue,
            ),
            scene_directive: BasicReasoningStyleOptimizer
                .directive_for(SceneReasoningStyle::CharacterDialogue),
            recent_messages: vec![],
            mode: None,
        });

        assert_eq!(
            context
                .player_memories
                .iter()
                .map(|memory| memory.id.as_str())
                .collect::<Vec<_>>(),
            vec!["p2", "p3", "p9", "p5", "p7", "p4", "p6", "p8"]
        );
        assert_eq!(
            context
                .gm_only_memories
                .iter()
                .map(|memory| memory.id.as_str())
                .collect::<Vec<_>>(),
            vec!["g2", "g3", "g5", "g4", "g1"]
        );
    }
}
