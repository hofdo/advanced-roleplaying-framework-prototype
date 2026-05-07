use crate::ValidatedWorldStateDelta;
use domain::{
    EntityRef, FactVisibility, FrontendStatePatch, FrontendVisibleState, NpcStatus, QuestStatus,
    Scenario, ViewerContext, VisibleClock, VisibleFact, VisibleLocation, VisibleNpc, VisibleQuest,
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
            .filter(|npc| {
                viewer.is_admin
                    || !matches!(
                        npc.status,
                        NpcStatus::Hidden | NpcStatus::Missing | NpcStatus::Unknown
                    )
            })
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
            changed_entities: changed_entities(delta),
            visible_state: Some(self.project(scenario, state, viewer)),
        }
    }
}

pub fn changed_entities(delta: &ValidatedWorldStateDelta) -> Vec<EntityRef> {
    let delta = &delta.0;
    let mut refs = Vec::new();

    for change in &delta.npc_changes {
        let id = match change {
            domain::NpcChange::AttitudeChanged { npc_id, .. }
            | domain::NpcChange::KnowledgeAdded { npc_id, .. }
            | domain::NpcChange::StatusChanged { npc_id, .. }
            | domain::NpcChange::LocationChanged { npc_id, .. } => npc_id,
        };
        push_unique(&mut refs, "npc", id);
    }
    for change in &delta.faction_changes {
        let id = match change {
            domain::FactionChange::StandingChanged { faction_id, .. }
            | domain::FactionChange::GoalRevealed { faction_id, .. } => faction_id,
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
            | domain::ClockChange::SetValue { clock_id, .. } => clock_id,
        };
        push_unique(&mut refs, "clock", id);
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
}
