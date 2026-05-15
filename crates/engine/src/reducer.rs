use crate::ValidatedWorldStateDelta;
use domain::{
    ActionResolution, ActionResolutionChange, ActiveSpeakerChange, ClockChange, ClueChange,
    ClueState, Fact, FactSource, FactionChange, InventoryChange, MemoryChange, MemoryEntry,
    NpcChange, OffscreenAction, PlayerChange, PlayerCondition, PlayerGoal, PlayerTrait,
    QuestChange, QuestStatus, RelationshipChange, RelationshipState, SceneChange, SummaryUpdate,
    WorldState,
};

pub trait WorldStateReducer: Send + Sync {
    fn apply(&self, state: WorldState, delta: ValidatedWorldStateDelta) -> WorldState;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicWorldStateReducer;

impl WorldStateReducer for BasicWorldStateReducer {
    fn apply(&self, mut state: WorldState, delta: ValidatedWorldStateDelta) -> WorldState {
        let delta = delta.0;

        if let Some(SceneChange { scene, .. }) = delta.scene_change {
            state.current_scene = scene;
        }

        if let Some(ActiveSpeakerChange { speaker_id, .. }) = delta.active_speaker_change {
            state.active_speaker_id = speaker_id;
        }

        if let Some(SummaryUpdate { summary, .. }) = delta.summary_update {
            state.summary = summary;
        }

        for change in delta.memory_changes {
            match change {
                MemoryChange::Added {
                    text,
                    visibility,
                    importance,
                    related_entity_ids,
                    ..
                } => {
                    let id = format!("memory-{}-{}", state.version + 1, state.memories.len() + 1);
                    state.memories.push(MemoryEntry {
                        id,
                        text,
                        visibility,
                        importance,
                        related_entity_ids,
                        source_message_id: None,
                    });
                }
                MemoryChange::ImportanceChanged {
                    memory_id,
                    importance,
                    ..
                } => {
                    if let Some(memory) = state
                        .memories
                        .iter_mut()
                        .find(|memory| memory.id == memory_id)
                    {
                        memory.importance = importance;
                    }
                }
            }
        }

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

        for change in delta.action_resolution_changes {
            match change {
                ActionResolutionChange::Recorded {
                    intent,
                    stakes,
                    outcome,
                    consequence,
                    visible_to_player,
                    linked_clock_ids,
                    ..
                } => {
                    let id = format!(
                        "action-{}-{}",
                        state.version + 1,
                        state.action_resolutions.len() + 1
                    );
                    state.action_resolutions.push(ActionResolution {
                        id,
                        intent,
                        stakes,
                        outcome,
                        consequence,
                        visible_to_player,
                        linked_clock_ids,
                    });
                }
            }
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
                    let fact_id = format!("fact-{}-{}", state.version + 1, state.facts.len() + 1);
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
                NpcChange::NoteAdded { npc_id, note, .. } => {
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.notes.push(note);
                    }
                }
                NpcChange::VisibilityChanged {
                    npc_id,
                    visible_to_player,
                    ..
                } => {
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.visible_to_player = visible_to_player;
                    }
                }
                NpcChange::AvailabilityChanged {
                    npc_id,
                    availability,
                    ..
                } => {
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.availability = availability;
                    }
                }
                NpcChange::IntentChanged { npc_id, intent, .. } => {
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.current_intent = intent;
                    }
                }
                NpcChange::OffscreenActionRecorded {
                    npc_id,
                    intent,
                    result,
                    visible_to_player,
                    ..
                } => {
                    if let Some(npc) = state.npcs.iter_mut().find(|npc| npc.npc_id == npc_id) {
                        npc.offscreen_actions.push(OffscreenAction {
                            intent,
                            result,
                            visible_to_player,
                        });
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
                FactionChange::PublicNoteAdded {
                    faction_id, note, ..
                } => {
                    if let Some(faction) = state
                        .factions
                        .iter_mut()
                        .find(|faction| faction.faction_id == faction_id)
                    {
                        faction.public_notes.push(note);
                    }
                }
                FactionChange::HiddenNoteAdded {
                    faction_id, note, ..
                } => {
                    if let Some(faction) = state
                        .factions
                        .iter_mut()
                        .find(|faction| faction.faction_id == faction_id)
                    {
                        faction.hidden_notes.push(note);
                    }
                }
                FactionChange::PressureChanged {
                    faction_id, delta, ..
                } => {
                    if let Some(faction) = state
                        .factions
                        .iter_mut()
                        .find(|faction| faction.faction_id == faction_id)
                    {
                        faction.pressure += delta;
                    }
                }
                FactionChange::PublicPressureNoteAdded {
                    faction_id, note, ..
                } => {
                    if let Some(faction) = state
                        .factions
                        .iter_mut()
                        .find(|faction| faction.faction_id == faction_id)
                    {
                        faction.public_pressure_notes.push(note);
                    }
                }
                FactionChange::HiddenPressureNoteAdded {
                    faction_id, note, ..
                } => {
                    if let Some(faction) = state
                        .factions
                        .iter_mut()
                        .find(|faction| faction.faction_id == faction_id)
                    {
                        faction.hidden_pressure_notes.push(note);
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
                        let next = (i16::from(clock.current) + i16::from(delta))
                            .clamp(0, i16::from(clock.max));
                        clock.current = next as u8;
                    }
                }
                ClockChange::SetValue {
                    clock_id, value, ..
                } => {
                    if let Some(clock) = state.clocks.iter_mut().find(|clock| clock.id == clock_id)
                    {
                        clock.current = value.min(clock.max);
                    }
                }
                ClockChange::VisibilityChanged {
                    clock_id,
                    visible_to_player,
                    ..
                } => {
                    if let Some(clock) = state.clocks.iter_mut().find(|clock| clock.id == clock_id)
                    {
                        clock.visible_to_player = visible_to_player;
                    }
                }
            }
        }

        for change in delta.inventory_changes {
            match change {
                InventoryChange::Added { item, .. } => {
                    if let Some(existing) =
                        state.inventory.iter_mut().find(|entry| entry.id == item.id)
                    {
                        *existing = item;
                    } else {
                        state.inventory.push(item);
                    }
                }
                InventoryChange::Removed { item_id, .. } => {
                    state.inventory.retain(|item| item.id != item_id);
                }
                InventoryChange::Updated { item, .. } => {
                    if let Some(existing) =
                        state.inventory.iter_mut().find(|entry| entry.id == item.id)
                    {
                        *existing = item;
                    }
                }
            }
        }

        for change in delta.player_changes {
            match change {
                PlayerChange::TraitAdded {
                    trait_id,
                    label,
                    description,
                    visible_to_player,
                    ..
                } => state.player.traits.push(PlayerTrait {
                    id: trait_id,
                    label,
                    description,
                    visible_to_player,
                }),
                PlayerChange::GoalAdded {
                    goal_id,
                    label,
                    description,
                    progress,
                    visible_to_player,
                    ..
                } => state.player.goals.push(PlayerGoal {
                    id: goal_id,
                    label,
                    description,
                    progress,
                    visible_to_player,
                }),
                PlayerChange::GoalProgressed { goal_id, delta, .. } => {
                    if let Some(goal) = state
                        .player
                        .goals
                        .iter_mut()
                        .find(|goal| goal.id == goal_id)
                    {
                        goal.progress += delta;
                    }
                }
                PlayerChange::ConditionAdded {
                    condition_id,
                    label,
                    description,
                    visible_to_player,
                    ..
                } => state.player.conditions.push(PlayerCondition {
                    id: condition_id,
                    label,
                    description,
                    visible_to_player,
                }),
                PlayerChange::ConditionCleared { condition_id, .. } => {
                    state
                        .player
                        .conditions
                        .retain(|condition| condition.id != condition_id);
                }
                PlayerChange::ResourceChanged {
                    resource_id, delta, ..
                } => {
                    if let Some(resource) = state
                        .player
                        .resources
                        .iter_mut()
                        .find(|resource| resource.id == resource_id)
                    {
                        resource.current += delta;
                    }
                }
                PlayerChange::GmNoteAdded { note, .. } => state.player.gm_notes.push(note),
            }
        }

        for change in delta.relationship_changes {
            match change {
                RelationshipChange::Changed {
                    source_id,
                    target_id,
                    attitude_delta,
                    ..
                } => {
                    find_or_insert_relationship_mut(&mut state, &source_id, &target_id).attitude +=
                        attitude_delta;
                }
                RelationshipChange::NoteAdded {
                    source_id,
                    target_id,
                    note,
                    ..
                } => {
                    find_or_insert_relationship_mut(&mut state, &source_id, &target_id)
                        .notes
                        .push(note);
                }
                RelationshipChange::TrustChanged {
                    source_id,
                    target_id,
                    delta,
                    ..
                } => {
                    find_or_insert_relationship_mut(&mut state, &source_id, &target_id).trust +=
                        delta;
                }
                RelationshipChange::SuspicionChanged {
                    source_id,
                    target_id,
                    delta,
                    ..
                } => {
                    find_or_insert_relationship_mut(&mut state, &source_id, &target_id)
                        .suspicion += delta;
                }
                RelationshipChange::LoyaltyChanged {
                    source_id,
                    target_id,
                    delta,
                    ..
                } => {
                    find_or_insert_relationship_mut(&mut state, &source_id, &target_id).loyalty +=
                        delta;
                }
            }
        }

        for change in delta.clue_changes {
            match change {
                ClueChange::Discovered {
                    clue_id,
                    text,
                    linked_secret_ids,
                    satisfied_reveal_conditions,
                    visible_to_player,
                    ..
                } => {
                    if let Some(existing) = state.clues.iter_mut().find(|clue| clue.id == clue_id) {
                        *existing = ClueState {
                            id: clue_id,
                            text,
                            linked_secret_ids,
                            satisfied_reveal_conditions,
                            visible_to_player,
                        };
                    } else {
                        state.clues.push(ClueState {
                            id: clue_id,
                            text,
                            linked_secret_ids,
                            satisfied_reveal_conditions,
                            visible_to_player,
                        });
                    }
                }
                ClueChange::VisibilityChanged {
                    clue_id,
                    visible_to_player,
                    ..
                } => {
                    if let Some(clue) = state.clues.iter_mut().find(|clue| clue.id == clue_id) {
                        clue.visible_to_player = visible_to_player;
                    }
                }
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

fn find_or_insert_relationship_mut<'a>(
    state: &'a mut WorldState,
    source_id: &str,
    target_id: &str,
) -> &'a mut RelationshipState {
    if let Some(index) = state.relationships.iter().position(|relationship| {
        relationship.source_id == source_id && relationship.target_id == target_id
    }) {
        return &mut state.relationships[index];
    }

    state.relationships.push(RelationshipState {
        source_id: source_id.into(),
        target_id: target_id.into(),
        attitude: 0,
        notes: vec![],
        trust: 0,
        suspicion: 0,
        loyalty: 0,
    });
    state
        .relationships
        .last_mut()
        .expect("relationship inserted")
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::fixtures;
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
                pressure: 0,
                public_pressure_notes: vec![],
                hidden_pressure_notes: vec![],
            }],
            quests: vec![],
            clocks: vec![ClockState {
                id: "fame".into(),
                title: "Fame".into(),
                current: 1,
                max: 6,
                consequence: "Notice.".into(),
                visible_to_player: true,
            }],
            action_resolutions: vec![],
            relationships: vec![],
            inventory: vec![],
            player: PlayerCharacterState::default(),
            clues: vec![],
            memories: vec![],
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
                availability: NpcAvailability::Present,
                current_intent: None,
                offscreen_actions: vec![],
            }],
            factions: vec![],
            quests: vec![],
            clocks: vec![],
            action_resolutions: vec![],
            relationships: vec![],
            inventory: vec![],
            player: PlayerCharacterState::default(),
            clues: vec![],
            memories: vec![],
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
        let scenario = fixtures::scenario().with_npc("npc-1", "NPC One").build();
        let mut state = fixtures::world_state(&scenario).with_version(0).build();
        state.npcs = vec![NpcState {
            npc_id: "npc-1".into(),
            status: NpcStatus::Active,
            visible_to_player: true,
            location_id: None,
            attitude_to_player: None,
            known_facts: vec![],
            notes: vec![],
            availability: NpcAvailability::Present,
            current_intent: None,
            offscreen_actions: vec![],
        }];
        state.factions.clear();
        state.quests.clear();
        state.clocks.clear();
        state.facts.clear();
        state
    }

    fn minimal_faction_state() -> WorldState {
        let scenario = fixtures::scenario().build();
        let mut state = fixtures::world_state(&scenario).with_version(0).build();
        state.npcs.clear();
        state.quests.clear();
        state.clocks.clear();
        state.facts.clear();
        state
    }

    fn minimal_quest_state() -> WorldState {
        let scenario = fixtures::scenario().build();
        let mut state = fixtures::world_state(&scenario).with_version(0).build();
        state.npcs.clear();
        state.factions.clear();
        state.clocks.clear();
        state.facts.clear();
        state
    }

    fn minimal_clock_state() -> WorldState {
        let scenario = fixtures::scenario().build();
        let mut state = fixtures::world_state(&scenario).with_version(0).build();
        state.npcs.clear();
        state.factions.clear();
        state.quests.clear();
        state.facts.clear();
        state.clocks = vec![ClockState {
            id: "fame".into(),
            title: "Fame".into(),
            current: 2,
            max: 6,
            consequence: "Notice.".into(),
            visible_to_player: true,
        }];
        state
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

        assert!(
            result.factions[0]
                .revealed_goals
                .contains(&"monitor calamity-levels".into())
        );
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

        assert!(
            result.quests[0]
                .completed_objectives
                .contains(&"sign-form".into())
        );
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
            trust: 0,
            suspicion: 0,
            loyalty: 0,
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
            action_resolutions: vec![],
            relationships: vec![],
            inventory: vec![],
            player: PlayerCharacterState::default(),
            clues: vec![],
            memories: vec![],
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

    #[test]
    fn reducer_updates_scene_summary_speaker_inventory_and_notes() {
        let state = WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 2,
            current_location_id: Some("guildhall".into()),
            current_scene: Some("dialogue".into()),
            active_speaker_id: Some("examiner".into()),
            facts: vec![],
            npcs: vec![NpcState {
                npc_id: "examiner".into(),
                status: NpcStatus::Active,
                visible_to_player: true,
                location_id: Some("guildhall".into()),
                attitude_to_player: None,
                known_facts: vec![],
                notes: vec![],
                availability: NpcAvailability::Present,
                current_intent: None,
                offscreen_actions: vec![],
            }],
            factions: vec![FactionState {
                faction_id: "guild".into(),
                standing: 0,
                public_notes: vec![],
                hidden_notes: vec![],
                revealed_goals: vec![],
                pressure: 0,
                public_pressure_notes: vec![],
                hidden_pressure_notes: vec![],
            }],
            quests: vec![],
            clocks: vec![ClockState {
                id: "panic".into(),
                title: "Panic".into(),
                current: 1,
                max: 6,
                consequence: "The guild locks down.".into(),
                visible_to_player: true,
            }],
            action_resolutions: vec![],
            relationships: vec![RelationshipState {
                source_id: "examiner".into(),
                target_id: "guild".into(),
                attitude: 1,
                notes: vec![],
                trust: 0,
                suspicion: 0,
                loyalty: 0,
            }],
            inventory: vec![],
            player: PlayerCharacterState::default(),
            clues: vec![],
            memories: vec![],
            summary: Some("Before the confrontation".into()),
            recent_events: vec![],
        };
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            scene_change: Some(SceneChange {
                scene: Some("combat".into()),
                reason: "Weapons were drawn.".into(),
            }),
            active_speaker_change: Some(ActiveSpeakerChange {
                speaker_id: Some("seraphyne".into()),
                reason: "She took over the scene.".into(),
            }),
            summary_update: Some(SummaryUpdate {
                summary: Some("Combat erupted after the accusation.".into()),
                reason: "Long-term context should retain the escalation.".into(),
            }),
            inventory_changes: vec![InventoryChange::Added {
                item: InventoryItem {
                    id: "ritual-knife".into(),
                    name: "Ritual Knife".into(),
                    description: "Warm and humming.".into(),
                    visible: true,
                },
                reason: "The player claimed it from the altar.".into(),
            }],
            faction_changes: vec![
                FactionChange::PublicNoteAdded {
                    faction_id: "guild".into(),
                    note: "Publicly warned the hall.".into(),
                    reason: "Public faction memory should persist.".into(),
                },
                FactionChange::HiddenNoteAdded {
                    faction_id: "guild".into(),
                    note: "Opened a covert inquiry.".into(),
                    reason: "Hidden faction memory should persist.".into(),
                },
            ],
            relationship_changes: vec![RelationshipChange::NoteAdded {
                source_id: "examiner".into(),
                target_id: "guild".into(),
                note: "The examiner now reports directly to the guild masters.".into(),
                reason: "Relationship memory should persist.".into(),
            }],
            clock_changes: vec![ClockChange::VisibilityChanged {
                clock_id: "panic".into(),
                visible_to_player: false,
                reason: "The countdown became hidden.".into(),
            }],
            npc_changes: vec![
                NpcChange::NoteAdded {
                    npc_id: "examiner".into(),
                    note: "Still suspects the player is unstable.".into(),
                    reason: "NPC memory should persist.".into(),
                },
                NpcChange::VisibilityChanged {
                    npc_id: "examiner".into(),
                    visible_to_player: false,
                    reason: "The examiner slipped out of player view.".into(),
                },
            ],
            ..WorldStateDelta::default()
        });

        let next = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(next.current_scene.as_deref(), Some("combat"));
        assert_eq!(next.active_speaker_id.as_deref(), Some("seraphyne"));
        assert_eq!(
            next.summary.as_deref(),
            Some("Combat erupted after the accusation.")
        );
        assert_eq!(next.inventory.len(), 1);
        assert_eq!(next.inventory[0].id, "ritual-knife");
        assert_eq!(
            next.npcs[0].notes,
            vec!["Still suspects the player is unstable."]
        );
        assert!(!next.npcs[0].visible_to_player);
        assert_eq!(
            next.factions[0].public_notes,
            vec!["Publicly warned the hall."]
        );
        assert_eq!(
            next.factions[0].hidden_notes,
            vec!["Opened a covert inquiry."]
        );
        assert_eq!(
            next.relationships[0].notes,
            vec!["The examiner now reports directly to the guild masters."]
        );
        assert!(!next.clocks[0].visible_to_player);
    }

    #[test]
    fn memory_added_creates_generated_id() {
        let state = fixtures::world_state(&fixtures::scenario().build())
            .with_version(2)
            .build();
        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            memory_changes: vec![MemoryChange::Added {
                text: "Marta remembers how the player treated the staff.".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 7,
                related_entity_ids: vec!["examiner".into()],
                reason: "This should persist for future scenes.".into(),
            }],
            ..WorldStateDelta::default()
        });

        let next = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(next.memories.len(), 1);
        assert_eq!(next.memories[0].id, "memory-3-1");
        assert_eq!(
            next.memories[0].text,
            "Marta remembers how the player treated the staff."
        );
        assert_eq!(next.memories[0].importance, 7);
    }

    #[test]
    fn importance_changed_updates_only_matching_memory() {
        let mut state = fixtures::world_state(&fixtures::scenario().build())
            .with_version(4)
            .build();
        state.memories = vec![
            MemoryEntry {
                id: "memory-4-1".into(),
                text: "The player impressed Marta.".into(),
                visibility: MemoryVisibility::PlayerKnown,
                importance: 3,
                related_entity_ids: vec!["examiner".into()],
                source_message_id: None,
            },
            MemoryEntry {
                id: "memory-4-2".into(),
                text: "The guild suspects the player.".into(),
                visibility: MemoryVisibility::GmOnly,
                importance: 8,
                related_entity_ids: vec!["guild".into()],
                source_message_id: None,
            },
        ];

        let delta = ValidatedWorldStateDelta(WorldStateDelta {
            memory_changes: vec![MemoryChange::ImportanceChanged {
                memory_id: "memory-4-1".into(),
                importance: 9,
                reason: "This memory became central.".into(),
            }],
            ..WorldStateDelta::default()
        });

        let next = BasicWorldStateReducer.apply(state, delta);

        assert_eq!(next.memories[0].importance, 9);
        assert_eq!(next.memories[1].importance, 8);
    }
}
