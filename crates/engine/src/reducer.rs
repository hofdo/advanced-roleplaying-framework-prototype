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
                related_secret_ids: fact.related_secret_ids.clone(),
                reveal_condition_satisfied: fact.reveal_condition_satisfied.clone(),
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
                NpcChange::KnowledgeAdded {
                    npc_id,
                    fact,
                    visibility,
                    ..
                } => {
                    let fact_id = format!(
                        "fact-{}-{}",
                        state.version + 1,
                        state.facts.len() + 1
                    );
                    state.facts.push(Fact {
                        id: fact_id.clone(),
                        text: fact,
                        visibility,
                        known_by: vec![npc_id.clone()],
                        source: FactSource::Turn,
                        reveal_conditions: vec![],
                        related_secret_ids: vec![],
                        reveal_condition_satisfied: None,
                    });
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.known_facts.push(fact_id);
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

    #[test]
    fn knowledge_added_creates_fact_and_registers_on_npc() {
        let npc_id = "npc-guard".to_string();
        let state = WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 1,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![],
            npcs: vec![NpcState {
                npc_id: npc_id.clone(),
                status: NpcStatus::Active,
                visible_to_player: true,
                location_id: None,
                attitude_to_player: None,
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
        };
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            npc_changes: vec![NpcChange::KnowledgeAdded {
                npc_id: npc_id.clone(),
                fact: "The vault key is hidden under the altar.".to_string(),
                visibility: FactVisibility::NpcKnown,
                reason: "Overheard conversation.".to_string(),
            }],
            ..WorldStateDelta::default()
        });

        let next = BasicWorldStateReducer.apply(state, delta);

        // A new Fact must have been created in world_state.facts
        assert_eq!(next.facts.len(), 1);
        let fact = &next.facts[0];
        assert_eq!(fact.text, "The vault key is hidden under the altar.");
        assert_eq!(fact.visibility, FactVisibility::NpcKnown);
        assert_eq!(fact.known_by, vec![npc_id.clone()]);
        assert_eq!(fact.source, FactSource::Turn);
        assert!(fact.reveal_conditions.is_empty());

        // The NPC's known_facts must contain the new fact's id
        let npc = next.npcs.iter().find(|n| n.npc_id == npc_id).unwrap();
        assert_eq!(npc.known_facts, vec![fact.id.clone()]);

        // npc.notes must remain empty (not written to)
        assert!(npc.notes.is_empty());
    }

    fn minimal_npc_state() -> WorldState {
        WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 0,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![],
            npcs: vec![NpcState {
                npc_id: "npc-1".into(),
                status: NpcStatus::Active,
                visible_to_player: true,
                location_id: None,
                attitude_to_player: None,
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

    fn minimal_faction_state() -> WorldState {
        WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 0,
            current_location_id: None,
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
            clocks: vec![],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        }
    }

    fn minimal_quest_state() -> WorldState {
        WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 0,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![],
            npcs: vec![],
            factions: vec![],
            quests: vec![QuestState {
                quest_id: "register".into(),
                status: QuestStatus::Available,
                completed_objectives: vec![],
                visible: true,
            }],
            clocks: vec![],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        }
    }

    fn minimal_clock_state() -> WorldState {
        WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 0,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![],
            npcs: vec![],
            factions: vec![],
            quests: vec![],
            clocks: vec![ClockState {
                id: "fame".into(),
                title: "Fame".into(),
                current: 2,
                max: 6,
                consequence: "Notice.".into(),
            }],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        }
    }

    #[test]
    fn npc_attitude_changed_updates_npc_field() {
        let state = minimal_npc_state();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            npc_changes: vec![NpcChange::AttitudeChanged {
                npc_id: "npc-1".into(),
                attitude: "hostile".into(),
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.npcs[0].attitude_to_player, Some("hostile".into()));
        assert_eq!(result.version, 1);
    }

    #[test]
    fn npc_status_changed_updates_npc_status() {
        let state = minimal_npc_state();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            npc_changes: vec![NpcChange::StatusChanged {
                npc_id: "npc-1".into(),
                status: NpcStatus::Unconscious,
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.npcs[0].status, NpcStatus::Unconscious);
    }

    #[test]
    fn npc_location_changed_updates_npc_location() {
        let state = minimal_npc_state();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            npc_changes: vec![NpcChange::LocationChanged {
                npc_id: "npc-1".into(),
                location_id: "tavern".into(),
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.npcs[0].location_id, Some("tavern".into()));
    }

    #[test]
    fn faction_goal_revealed_appends_to_revealed_goals() {
        let state = minimal_faction_state();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            faction_changes: vec![FactionChange::GoalRevealed {
                faction_id: "guild".into(),
                goal: "monitor calamity-levels".into(),
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert!(result.factions[0]
            .revealed_goals
            .contains(&"monitor calamity-levels".into()));
    }

    #[test]
    fn quest_started_sets_status_to_active() {
        let state = minimal_quest_state();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            quest_changes: vec![QuestChange::Started {
                quest_id: "register".into(),
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.quests[0].status, QuestStatus::Active);
    }

    #[test]
    fn quest_objective_completed_appends_to_completed_objectives() {
        let state = minimal_quest_state();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            quest_changes: vec![QuestChange::ObjectiveCompleted {
                quest_id: "register".into(),
                objective_id: "sign-form".into(),
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert!(result.quests[0]
            .completed_objectives
            .contains(&"sign-form".into()));
    }

    #[test]
    fn quest_completed_sets_status_to_completed() {
        let state = minimal_quest_state();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            quest_changes: vec![QuestChange::Completed {
                quest_id: "register".into(),
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.quests[0].status, QuestStatus::Completed);
    }

    #[test]
    fn quest_failed_sets_status_to_failed() {
        let state = minimal_quest_state();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            quest_changes: vec![QuestChange::Failed {
                quest_id: "register".into(),
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.quests[0].status, QuestStatus::Failed);
    }

    #[test]
    fn clock_set_value_replaces_current() {
        let state = minimal_clock_state();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            clock_changes: vec![ClockChange::SetValue {
                clock_id: "fame".into(),
                value: 5,
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.clocks[0].current, 5);
    }

    #[test]
    fn relationship_changed_creates_new_entry() {
        let mut state = minimal_clock_state();
        state.relationships = vec![];
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            relationship_changes: vec![RelationshipChange::Changed {
                source_id: "player".into(),
                target_id: "examiner".into(),
                attitude_delta: 10,
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.relationships.len(), 1);
        assert_eq!(result.relationships[0].attitude, 10);
        assert_eq!(result.relationships[0].source_id, "player");
        assert_eq!(result.relationships[0].target_id, "examiner");
    }

    #[test]
    fn relationship_changed_updates_existing_entry() {
        let mut state = minimal_clock_state();
        state.relationships = vec![RelationshipState {
            source_id: "player".into(),
            target_id: "examiner".into(),
            attitude: 5,
            notes: vec![],
        }];
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            relationship_changes: vec![RelationshipChange::Changed {
                source_id: "player".into(),
                target_id: "examiner".into(),
                attitude_delta: 3,
                reason: "x".into(),
            }],
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.relationships.len(), 1);
        assert_eq!(result.relationships[0].attitude, 8);
    }

    #[test]
    fn location_change_updates_current_location() {
        let state = WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 0,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![],
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
            location_change: Some(LocationChange {
                location_id: "dungeon".into(),
                reason: "x".into(),
            }),
            ..WorldStateDelta::default()
        });

        let result = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(result.current_location_id, Some("dungeon".into()));
    }
}
