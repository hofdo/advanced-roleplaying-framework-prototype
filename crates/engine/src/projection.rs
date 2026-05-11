use crate::ValidatedWorldStateDelta;
use domain::{
    EntityRef, FactVisibility, FrontendStatePatch, FrontendVisibleState, QuestStatus, Scenario,
    ViewerContext, VisibleClock, VisibleFact, VisibleLocation, VisibleNpc, VisibleQuest,
    WorldState,
};

pub trait FrontendStateProjector: Send + Sync {
    fn project(
        &self,
        scenario: &Scenario,
        state: &WorldState,
        viewer: &ViewerContext,
    ) -> FrontendVisibleState;

    fn patch_from_delta(
        &self,
        scenario: &Scenario,
        state: &WorldState,
        delta: &ValidatedWorldStateDelta,
        viewer: &ViewerContext,
    ) -> FrontendStatePatch;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicFrontendStateProjector;

impl FrontendStateProjector for BasicFrontendStateProjector {
    fn project(
        &self,
        scenario: &Scenario,
        state: &WorldState,
        viewer: &ViewerContext,
    ) -> FrontendVisibleState {
        let current_location = state.current_location_id.as_ref().and_then(|id| {
            scenario
                .locations
                .iter()
                .find(|location| &location.id == id && (location.visible || viewer.is_admin))
                .map(|location| VisibleLocation {
                    id: location.id.clone(),
                    name: location.name.clone(),
                    description: location.description.clone(),
                })
        });

        let visible_npcs = state
            .npcs
            .iter()
            .filter(|npc| viewer.is_admin || npc.visible_to_player)
            .filter_map(|npc_state| {
                scenario
                    .npcs
                    .iter()
                    .find(|npc| npc.id == npc_state.npc_id)
                    .map(|npc| VisibleNpc {
                        id: npc.id.clone(),
                        name: npc.name.clone(),
                        description: npc.description.clone(),
                        status: npc_state.status,
                        attitude_to_player: npc_state.attitude_to_player.clone(),
                    })
            })
            .collect::<Vec<_>>();

        let active_speaker = state
            .active_speaker_id
            .as_ref()
            .and_then(|id| visible_npcs.iter().find(|npc| &npc.id == id).cloned());

        let visible_quests = state
            .quests
            .iter()
            .filter(|quest| {
                viewer.is_admin || (quest.visible && quest.status != QuestStatus::Hidden)
            })
            .map(|quest| VisibleQuest {
                id: quest.quest_id.clone(),
                status: quest.status,
            })
            .collect();

        let visible_clocks = state
            .clocks
            .iter()
            .filter(|clock| viewer.is_admin || clock.visible_to_player)
            .map(|clock| VisibleClock {
                id: clock.id.clone(),
                title: clock.title.clone(),
                current: clock.current,
                max: clock.max,
                consequence: clock.consequence.clone(),
            })
            .collect();

        let player_known_facts = state
            .facts
            .iter()
            .filter(|fact| fact.visibility == FactVisibility::PlayerKnown || viewer.is_admin)
            .map(|fact| VisibleFact {
                id: fact.id.clone(),
                text: fact.text.clone(),
            })
            .collect();

        FrontendVisibleState {
            state_version: state.version,
            current_location,
            active_speaker,
            visible_npcs,
            visible_quests,
            visible_clocks,
            player_known_facts,
            recent_public_events: state.recent_events.clone(),
        }
    }

    fn patch_from_delta(
        &self,
        scenario: &Scenario,
        state: &WorldState,
        delta: &ValidatedWorldStateDelta,
        viewer: &ViewerContext,
    ) -> FrontendStatePatch {
        FrontendStatePatch {
            state_version: state.version,
            changed_entities: changed_entities(state, delta),
            visible_state: Some(self.project(scenario, state, viewer)),
        }
    }
}

pub fn changed_entities(state: &WorldState, delta: &ValidatedWorldStateDelta) -> Vec<EntityRef> {
    let delta = &delta.0;
    let mut refs = Vec::new();

    for fact in state
        .facts
        .iter()
        .rev()
        .take(delta.facts_to_add.len())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        push_unique(&mut refs, "fact", &fact.id);
    }

    for change in &delta.npc_changes {
        let id = match change {
            domain::NpcChange::AttitudeChanged { npc_id, .. }
            | domain::NpcChange::KnowledgeAdded { npc_id, .. }
            | domain::NpcChange::StatusChanged { npc_id, .. }
            | domain::NpcChange::LocationChanged { npc_id, .. }
            | domain::NpcChange::NoteAdded { npc_id, .. } => npc_id,
        };
        push_unique(&mut refs, "npc", id);
    }
    for change in &delta.faction_changes {
        let id = match change {
            domain::FactionChange::StandingChanged { faction_id, .. }
            | domain::FactionChange::GoalRevealed { faction_id, .. }
            | domain::FactionChange::PublicNoteAdded { faction_id, .. }
            | domain::FactionChange::HiddenNoteAdded { faction_id, .. } => faction_id,
        };
        push_unique(&mut refs, "faction", id);
    }
    for change in &delta.quest_changes {
        let id = match change {
            domain::QuestChange::Started { quest_id, .. }
            | domain::QuestChange::ObjectiveCompleted { quest_id, .. }
            | domain::QuestChange::Completed { quest_id, .. }
            | domain::QuestChange::Failed { quest_id, .. } => quest_id,
        };
        push_unique(&mut refs, "quest", id);
    }
    for change in &delta.clock_changes {
        let id = match change {
            domain::ClockChange::Advanced { clock_id, .. }
            | domain::ClockChange::SetValue { clock_id, .. }
            | domain::ClockChange::VisibilityChanged { clock_id, .. } => clock_id,
        };
        push_unique(&mut refs, "clock", id);
    }
    for change in &delta.relationship_changes {
        let id = match change {
            domain::RelationshipChange::Changed {
                source_id,
                target_id,
                ..
            }
            | domain::RelationshipChange::NoteAdded {
                source_id,
                target_id,
                ..
            } => format!("{source_id}->{target_id}"),
        };
        push_unique(&mut refs, "relationship", &id);
    }
    if let Some(location) = &delta.location_change {
        push_unique(&mut refs, "location", &location.location_id);
    }

    refs
}

fn push_unique(refs: &mut Vec<EntityRef>, entity_type: &str, id: &str) {
    if !refs
        .iter()
        .any(|item| item.entity_type == entity_type && item.id == id)
    {
        refs.push(EntityRef {
            entity_type: entity_type.into(),
            id: id.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::*;
    use uuid::Uuid;

    #[test]
    fn projection_filters_gm_only_facts_for_normal_viewers() {
        let scenario = Scenario {
            id: Uuid::new_v4(),
            title: "Aurethia".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "fantasy".into(),
            tone: "heroic".into(),
            rules: vec![],
            locations: vec![],
            factions: vec![],
            npcs: vec![],
            quests: vec![],
            secrets: vec![],
            clocks: vec![],
        };
        let state = WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: scenario.id,
            version: 2,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![
                Fact {
                    id: "known".into(),
                    text: "The goddess is worried.".into(),
                    visibility: FactVisibility::PlayerKnown,
                    known_by: vec![],
                    source: FactSource::Scenario,
                    reveal_conditions: vec![],
                    related_secret_ids: vec![],
                    reveal_condition_satisfied: None,
                },
                Fact {
                    id: "secret".into(),
                    text: "The mark came from the void.".into(),
                    visibility: FactVisibility::GmOnly,
                    known_by: vec![],
                    source: FactSource::Scenario,
                    reveal_conditions: vec![],
                    related_secret_ids: vec![],
                    reveal_condition_satisfied: None,
                },
            ],
            npcs: vec![],
            factions: vec![],
            quests: vec![],
            clocks: vec![],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        };

        let projected =
            BasicFrontendStateProjector.project(&scenario, &state, &ViewerContext::player());

        assert_eq!(projected.player_known_facts.len(), 1);
        assert_eq!(projected.player_known_facts[0].id, "known");
    }

    fn make_scenario_with_npc(npc_id: &str) -> Scenario {
        Scenario {
            id: Uuid::new_v4(),
            title: "Test".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "test".into(),
            tone: "neutral".into(),
            rules: vec![],
            locations: vec![],
            factions: vec![],
            npcs: vec![Npc {
                id: npc_id.into(),
                name: "Test NPC".into(),
                description: "A test NPC.".into(),
                role_identity: RoleIdentity {
                    core_emotion: "neutral".into(),
                    motivation: "exist".into(),
                    worldview: "ordinary".into(),
                    fear: None,
                    desire: None,
                    speech_style: "plain".into(),
                    boundaries: vec![],
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

    fn make_world_state(scenario: &Scenario, npc_state: NpcState) -> WorldState {
        WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: scenario.id,
            version: 1,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![],
            npcs: vec![npc_state],
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
    fn npc_with_visible_to_player_false_hidden_from_projection() {
        let scenario = make_scenario_with_npc("villain");
        let state = make_world_state(
            &scenario,
            NpcState {
                npc_id: "villain".into(),
                status: NpcStatus::Active,
                visible_to_player: false,
                location_id: None,
                attitude_to_player: None,
                known_facts: vec![],
                notes: vec![],
            },
        );

        let projected =
            BasicFrontendStateProjector.project(&scenario, &state, &ViewerContext::player());

        assert!(
            projected.visible_npcs.is_empty(),
            "NPC with visible_to_player=false must not appear in visible_npcs"
        );
    }

    #[test]
    fn npc_missing_but_visible_to_player_shows_in_projection() {
        let scenario = make_scenario_with_npc("ally");
        let state = make_world_state(
            &scenario,
            NpcState {
                npc_id: "ally".into(),
                status: NpcStatus::Missing,
                visible_to_player: true,
                location_id: None,
                attitude_to_player: None,
                known_facts: vec![],
                notes: vec![],
            },
        );

        let projected =
            BasicFrontendStateProjector.project(&scenario, &state, &ViewerContext::player());

        assert_eq!(
            projected.visible_npcs.len(),
            1,
            "NPC with visible_to_player=true must appear in visible_npcs even when Missing"
        );
        assert_eq!(projected.visible_npcs[0].id, "ally");
        assert_eq!(projected.visible_npcs[0].status, NpcStatus::Missing);
    }

    #[test]
    fn hidden_clocks_are_filtered_for_players_but_visible_to_admins() {
        let scenario = Scenario {
            id: Uuid::new_v4(),
            title: "Test".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "test".into(),
            tone: "neutral".into(),
            rules: vec![],
            locations: vec![],
            factions: vec![],
            npcs: vec![],
            quests: vec![],
            secrets: vec![],
            clocks: vec![],
        };
        let state = WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: scenario.id,
            version: 1,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![],
            npcs: vec![],
            factions: vec![],
            quests: vec![],
            clocks: vec![
                ClockState {
                    id: "public".into(),
                    title: "Public Clock".into(),
                    current: 1,
                    max: 4,
                    consequence: "Everyone knows.".into(),
                    visible_to_player: true,
                },
                ClockState {
                    id: "hidden".into(),
                    title: "Hidden Clock".into(),
                    current: 2,
                    max: 6,
                    consequence: "GM only.".into(),
                    visible_to_player: false,
                },
            ],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        };

        let player =
            BasicFrontendStateProjector.project(&scenario, &state, &ViewerContext::player());
        let admin = BasicFrontendStateProjector.project(
            &scenario,
            &state,
            &ViewerContext {
                include_debug_state: true,
                is_admin: true,
            },
        );

        assert_eq!(player.visible_clocks.len(), 1);
        assert_eq!(player.visible_clocks[0].id, "public");
        assert_eq!(admin.visible_clocks.len(), 2);
    }

    #[test]
    fn admin_projection_includes_gm_only_facts() {
        let scenario = Scenario {
            id: Uuid::new_v4(),
            title: "Aurethia".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "fantasy".into(),
            tone: "heroic".into(),
            rules: vec![],
            locations: vec![],
            factions: vec![],
            npcs: vec![],
            quests: vec![],
            secrets: vec![],
            clocks: vec![],
        };
        let state = WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: scenario.id,
            version: 1,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![
                Fact {
                    id: "known".into(),
                    text: "The goddess is worried.".into(),
                    visibility: FactVisibility::PlayerKnown,
                    known_by: vec![],
                    source: FactSource::Scenario,
                    reveal_conditions: vec![],
                    related_secret_ids: vec![],
                    reveal_condition_satisfied: None,
                },
                Fact {
                    id: "secret".into(),
                    text: "The mark came from the void.".into(),
                    visibility: FactVisibility::GmOnly,
                    known_by: vec![],
                    source: FactSource::Scenario,
                    reveal_conditions: vec![],
                    related_secret_ids: vec![],
                    reveal_condition_satisfied: None,
                },
            ],
            npcs: vec![],
            factions: vec![],
            quests: vec![],
            clocks: vec![],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        };

        let player_view =
            BasicFrontendStateProjector.project(&scenario, &state, &ViewerContext::player());
        let admin_view = BasicFrontendStateProjector.project(
            &scenario,
            &state,
            &ViewerContext {
                is_admin: true,
                include_debug_state: true,
            },
        );

        assert_eq!(player_view.player_known_facts.len(), 1);
        assert_eq!(player_view.player_known_facts[0].id, "known");
        assert_eq!(admin_view.player_known_facts.len(), 2);
        assert!(
            admin_view
                .player_known_facts
                .iter()
                .any(|f| f.id == "secret")
        );
    }

    #[test]
    fn changed_entities_include_added_facts_and_relationships() {
        let state = WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 2,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![Fact {
                id: "fact-2-1".into(),
                text: "The ritual knife is cursed".into(),
                visibility: FactVisibility::PlayerKnown,
                known_by: vec![],
                source: FactSource::Turn,
                reveal_conditions: vec![],
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            }],
            npcs: vec![],
            factions: vec![],
            quests: vec![],
            clocks: vec![],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        };
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            facts_to_add: vec![FactToAdd {
                text: "The ritual knife is cursed".into(),
                visibility: FactVisibility::PlayerKnown,
                known_by: vec![],
                reveal_conditions: vec![],
                reason: "The player inspected it".into(),
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            }],
            relationship_changes: vec![RelationshipChange::NoteAdded {
                source_id: "npc-1".into(),
                target_id: "npc-2".into(),
                note: "Tension remains high".into(),
                reason: "The argument left a mark".into(),
            }],
            ..WorldStateDelta::default()
        });

        let refs = changed_entities(&state, &delta);

        assert!(
            refs.iter()
                .any(|entity| entity.entity_type == "fact" && entity.id == "fact-2-1"),
            "facts_to_add should produce a fact entity ref"
        );
        assert!(
            refs.iter().any(|entity| {
                entity.entity_type == "relationship" && entity.id == "npc-1->npc-2"
            }),
            "relationship note changes should produce a relationship entity ref"
        );
    }
}
