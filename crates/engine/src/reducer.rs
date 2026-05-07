use crate::ValidatedWorldStateDelta;
use domain::{
    ClockChange, Fact, FactSource, FactionChange, NpcChange, QuestChange, QuestStatus,
    RelationshipChange, RelationshipState, WorldState,
};

pub trait WorldStateReducer: Send + Sync {
    fn apply(&self, state: WorldState, delta: ValidatedWorldStateDelta) -> WorldState;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicWorldStateReducer;

impl WorldStateReducer for BasicWorldStateReducer {
    fn apply(&self, mut state: WorldState, delta: ValidatedWorldStateDelta) -> WorldState {
        let delta = delta.0;

        for fact in delta.facts_to_add {
            let id = format!("fact-{}-{}", state.version + 1, state.facts.len() + 1);
            state.facts.push(Fact {
                id,
                text: fact.text,
                visibility: fact.visibility,
                known_by: fact.known_by,
                source: FactSource::Turn,
                reveal_conditions: fact.reveal_conditions,
            });
        }

        for change in delta.npc_changes {
            match change {
                NpcChange::AttitudeChanged {
                    npc_id, attitude, ..
                } => {
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.attitude_to_player = Some(attitude);
                    }
                }
                NpcChange::KnowledgeAdded { npc_id, fact, .. } => {
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.notes.push(fact);
                    }
                }
                NpcChange::StatusChanged { npc_id, status, .. } => {
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.status = status;
                    }
                }
                NpcChange::LocationChanged {
                    npc_id,
                    location_id,
                    ..
                } => {
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.location_id = Some(location_id);
                    }
                }
            }
        }

        for change in delta.faction_changes {
            match change {
                FactionChange::StandingChanged {
                    faction_id,
                    standing_delta,
                    ..
                } => {
                    if let Some(faction) = state
                        .factions
                        .iter_mut()
                        .find(|faction| faction.faction_id == faction_id)
                    {
                        faction.standing += standing_delta;
                    }
                }
                FactionChange::GoalRevealed {
                    faction_id, goal, ..
                } => {
                    if let Some(faction) = state
                        .factions
                        .iter_mut()
                        .find(|faction| faction.faction_id == faction_id)
                    {
                        faction.revealed_goals.push(goal);
                    }
                }
            }
        }

        for change in delta.quest_changes {
            match change {
                QuestChange::Started { quest_id, .. } => {
                    if let Some(quest) = state
                        .quests
                        .iter_mut()
                        .find(|quest| quest.quest_id == quest_id)
                    {
                        quest.status = QuestStatus::Active;
                    }
                }
                QuestChange::ObjectiveCompleted {
                    quest_id,
                    objective_id,
                    ..
                } => {
                    if let Some(quest) = state
                        .quests
                        .iter_mut()
                        .find(|quest| quest.quest_id == quest_id)
                    {
                        if !quest.completed_objectives.contains(&objective_id) {
                            quest.completed_objectives.push(objective_id);
                        }
                    }
                }
                QuestChange::Completed { quest_id, .. } => {
                    if let Some(quest) = state
                        .quests
                        .iter_mut()
                        .find(|quest| quest.quest_id == quest_id)
                    {
                        quest.status = QuestStatus::Completed;
                    }
                }
                QuestChange::Failed { quest_id, .. } => {
                    if let Some(quest) = state
                        .quests
                        .iter_mut()
                        .find(|quest| quest.quest_id == quest_id)
                    {
                        quest.status = QuestStatus::Failed;
                    }
                }
            }
        }

        for change in delta.clock_changes {
            match change {
                ClockChange::Advanced {
                    clock_id, delta, ..
                } => {
                    if let Some(clock) = state.clocks.iter_mut().find(|clock| clock.id == clock_id)
                    {
                        clock.current = (clock.current as i16 + delta as i16) as u8;
                    }
                }
                ClockChange::SetValue {
                    clock_id, value, ..
                } => {
                    if let Some(clock) = state.clocks.iter_mut().find(|clock| clock.id == clock_id)
                    {
                        clock.current = value;
                    }
                }
            }
        }

        for change in delta.relationship_changes {
            let RelationshipChange::Changed {
                source_id,
                target_id,
                attitude_delta,
                ..
            } = change;
            if let Some(relationship) = state.relationships.iter_mut().find(|relationship| {
                relationship.source_id == source_id && relationship.target_id == target_id
            }) {
                relationship.attitude += attitude_delta;
            } else {
                state.relationships.push(RelationshipState {
                    source_id,
                    target_id,
                    attitude: attitude_delta,
                    notes: vec![],
                });
            }
        }

        if let Some(location) = delta.location_change {
            state.current_location_id = Some(location.location_id);
        }

        state.recent_events.extend(delta.event_log_entries);
        state.version += 1;
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::*;
    use uuid::Uuid;

    #[test]
    fn reducer_applies_validated_delta_and_increments_version_once() {
        let state = WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 4,
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
                title: "Fame".into(),
                current: 1,
                max: 6,
                consequence: "Notice.".into(),
            }],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        };
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            faction_changes: vec![FactionChange::StandingChanged {
                faction_id: "guild".into(),
                standing_delta: -5,
                reason: "Panic in the hall.".into(),
            }],
            clock_changes: vec![ClockChange::Advanced {
                clock_id: "fame".into(),
                delta: 1,
                reason: "Many witnesses.".into(),
            }],
            event_log_entries: vec!["Mana surged in the guildhall.".into()],
            ..WorldStateDelta::default()
        });

        let next = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(next.version, 5);
        assert_eq!(next.factions[0].standing, -5);
        assert_eq!(next.clocks[0].current, 2);
        assert_eq!(next.recent_events, vec!["Mana surged in the guildhall."]);
    }
}
