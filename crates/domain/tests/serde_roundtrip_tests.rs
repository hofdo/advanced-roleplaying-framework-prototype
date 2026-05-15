use domain::{
    ActionOutcome, ActionResolutionChange, ClockChange, ConditionRef, Fact, FactVisibility,
    FactionChange, InventoryChange, InventoryItem, MatchMode, MemoryChange, MemoryVisibility,
    NpcAvailability, NpcChange, NpcState, NpcStatus, PlayerChange, QuestChange, RelationshipChange,
    RevealCondition, SceneReasoningStyle, TurnMode, WorldState,
};
use serde_json::json;

#[test]
fn world_state_without_memories_defaults_to_empty() {
    let state: WorldState = serde_json::from_value(json!({
        "session_id": "00000000-0000-0000-0000-000000000001",
        "scenario_id": "00000000-0000-0000-0000-000000000002",
        "version": 0,
        "current_location_id": null,
        "current_scene": null,
        "active_speaker_id": null,
        "facts": [],
        "npcs": [],
        "factions": [],
        "quests": [],
        "clocks": [],
        "relationships": [],
        "inventory": [],
        "summary": null,
        "recent_events": []
    }))
    .unwrap();

    assert!(state.memories.is_empty());
}

#[test]
fn world_state_without_gameplay_extensions_defaults_cleanly() {
    let state: WorldState = serde_json::from_value(json!({
        "session_id": "00000000-0000-0000-0000-000000000001",
        "scenario_id": "00000000-0000-0000-0000-000000000002",
        "version": 0,
        "current_location_id": null,
        "current_scene": null,
        "active_speaker_id": null,
        "facts": [],
        "npcs": [],
        "factions": [],
        "quests": [],
        "clocks": [],
        "relationships": [],
        "inventory": [],
        "summary": null,
        "recent_events": []
    }))
    .unwrap();

    assert!(state.action_resolutions.is_empty());
    assert!(state.clues.is_empty());
    assert!(state.player.traits.is_empty());
    assert!(state.player.goals.is_empty());
    assert!(state.player.conditions.is_empty());
    assert!(state.player.resources.is_empty());
}

#[test]
fn legacy_relationship_and_faction_state_default_new_social_fields() {
    let state: WorldState = serde_json::from_value(json!({
        "session_id": "00000000-0000-0000-0000-000000000001",
        "scenario_id": "00000000-0000-0000-0000-000000000002",
        "version": 0,
        "current_location_id": null,
        "current_scene": null,
        "active_speaker_id": null,
        "facts": [],
        "npcs": [],
        "factions": [{
            "faction_id": "imperial-throne",
            "standing": 5,
            "public_notes": [],
            "hidden_notes": [],
            "revealed_goals": []
        }],
        "quests": [],
        "clocks": [],
        "relationships": [{
            "source_id": "archduke-severin",
            "target_id": "player",
            "attitude": 3,
            "notes": ["watchful"]
        }],
        "inventory": [],
        "summary": null,
        "recent_events": []
    }))
    .unwrap();

    assert_eq!(state.relationships[0].trust, 0);
    assert_eq!(state.relationships[0].suspicion, 0);
    assert_eq!(state.relationships[0].loyalty, 0);
    assert_eq!(state.factions[0].pressure, 0);
    assert!(state.factions[0].public_pressure_notes.is_empty());
    assert!(state.factions[0].hidden_pressure_notes.is_empty());
}

#[test]
fn legacy_npc_state_defaults_availability_and_empty_agency_fields() {
    let npc: NpcState = serde_json::from_value(json!({
        "npc_id": "archduke-severin",
        "status": "active",
        "visible_to_player": true,
        "location_id": "frostmere-citadel",
        "attitude_to_player": null,
        "known_facts": [],
        "notes": []
    }))
    .unwrap();

    assert_eq!(npc.availability, NpcAvailability::Present);
    assert!(npc.current_intent.is_none());
    assert!(npc.offscreen_actions.is_empty());
}

#[test]
fn action_resolution_change_recorded_roundtrips() {
    let change: ActionResolutionChange = serde_json::from_value(json!({
        "type": "recorded",
        "intent": "Disarm the assassin before he reaches Marta.",
        "stakes": ["Marta may be injured", "the crowd may panic"],
        "outcome": "success_with_cost",
        "consequence": "The assassin is stopped, but the alarm clock advances.",
        "visible_to_player": true,
        "linked_clock_ids": ["wedding"],
        "reason": "The player chose a risky public action."
    }))
    .unwrap();

    assert_eq!(
        change,
        ActionResolutionChange::Recorded {
            intent: "Disarm the assassin before he reaches Marta.".into(),
            stakes: vec!["Marta may be injured".into(), "the crowd may panic".into()],
            outcome: ActionOutcome::SuccessWithCost,
            consequence: "The assassin is stopped, but the alarm clock advances.".into(),
            visible_to_player: true,
            linked_clock_ids: vec!["wedding".into()],
            reason: "The player chose a risky public action.".into(),
        }
    );
}

#[test]
fn player_character_resource_change_roundtrips() {
    let change: PlayerChange = serde_json::from_value(json!({
        "type": "resource_changed",
        "resource_id": "resolve",
        "delta": -1,
        "reason": "Elowen forced herself to stand firm in public."
    }))
    .unwrap();

    assert_eq!(
        change,
        PlayerChange::ResourceChanged {
            resource_id: "resolve".into(),
            delta: -1,
            reason: "Elowen forced herself to stand firm in public.".into(),
        }
    );
}

#[test]
fn trust_and_pressure_changes_roundtrip() {
    let trust: RelationshipChange = serde_json::from_value(json!({
        "type": "trust_changed",
        "source_id": "archduke-severin",
        "target_id": "player",
        "delta": 2,
        "reason": "Elowen defended Falkenmark publicly."
    }))
    .unwrap();
    let pressure: FactionChange = serde_json::from_value(json!({
        "type": "pressure_changed",
        "faction_id": "imperial-throne",
        "delta": 5,
        "public": true,
        "reason": "The emperor expects progress before the wedding."
    }))
    .unwrap();

    assert_eq!(
        trust,
        RelationshipChange::TrustChanged {
            source_id: "archduke-severin".into(),
            target_id: "player".into(),
            delta: 2,
            reason: "Elowen defended Falkenmark publicly.".into(),
        }
    );
    assert_eq!(
        pressure,
        FactionChange::PressureChanged {
            faction_id: "imperial-throne".into(),
            delta: 5,
            public: true,
            reason: "The emperor expects progress before the wedding.".into(),
        }
    );
}

#[test]
fn reveal_condition_and_condition_ref_roundtrip() {
    let condition: RevealCondition = serde_json::from_value(json!({
        "id": "inspect-treaty-seal",
        "description": "Player physically inspects the treaty seal."
    }))
    .unwrap();
    let reference: ConditionRef = serde_json::from_value(json!({
        "id": "inspect-treaty-seal",
        "mode": "exact"
    }))
    .unwrap();

    assert_eq!(
        condition,
        RevealCondition {
            id: "inspect-treaty-seal".into(),
            description: "Player physically inspects the treaty seal.".into(),
        }
    );
    assert_eq!(
        reference,
        ConditionRef {
            id: "inspect-treaty-seal".into(),
            mode: MatchMode::Exact,
        }
    );
}

#[test]
fn scenario_secret_with_structured_reveal_conditions_roundtrips() {
    let secret: domain::Secret = serde_json::from_value(json!({
        "id": "poisoned-treaty",
        "text": "The chancellor poisoned the treaty.",
        "reveal_conditions": [
            {
                "id": "inspect-treaty-seal",
                "description": "Player physically inspects the treaty seal."
            },
            {
                "id": "interrogate-chancellor",
                "description": "Player extracts a confession from the chancellor."
            }
        ]
    }))
    .unwrap();

    assert_eq!(secret.reveal_conditions.len(), 2);
    assert_eq!(secret.reveal_conditions[0].id, "inspect-treaty-seal");
}

#[test]
fn memory_change_added_roundtrips() {
    let change: MemoryChange = serde_json::from_value(json!({
        "type": "added",
        "text": "Elowen learned Marta judges nobles by how they treat servants.",
        "visibility": "player_known",
        "importance": 7,
        "related_entity_ids": ["steward-marta"],
        "reason": "The player spoke respectfully to staff."
    }))
    .unwrap();

    assert_eq!(
        change,
        MemoryChange::Added {
            text: "Elowen learned Marta judges nobles by how they treat servants.".into(),
            visibility: MemoryVisibility::PlayerKnown,
            importance: 7,
            related_entity_ids: vec!["steward-marta".into()],
            reason: "The player spoke respectfully to staff.".into(),
        }
    );

    let json = serde_json::to_string(&change).unwrap();
    let round: MemoryChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"added""#));
}

// ── NpcChange ────────────────────────────────────────────────────────────────

#[test]
fn npc_attitude_changed_roundtrips() {
    let change = NpcChange::AttitudeChanged {
        npc_id: "npc-1".into(),
        attitude: "hostile".into(),
        reason: "provoked".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: NpcChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"attitude_changed""#));
}

#[test]
fn npc_knowledge_added_roundtrips() {
    let change = NpcChange::KnowledgeAdded {
        npc_id: "npc-2".into(),
        fact: "knows the secret passage".into(),
        visibility: FactVisibility::NpcKnown,
        reason: "overheard conversation".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: NpcChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"knowledge_added""#));
}

#[test]
fn npc_status_changed_roundtrips() {
    let change = NpcChange::StatusChanged {
        npc_id: "npc-3".into(),
        status: NpcStatus::Injured,
        reason: "struck by arrow".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: NpcChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"status_changed""#));
}

#[test]
fn npc_location_changed_roundtrips() {
    let change = NpcChange::LocationChanged {
        npc_id: "npc-4".into(),
        location_id: "tavern".into(),
        reason: "fled the battle".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: NpcChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"location_changed""#));
}

#[test]
fn npc_note_added_roundtrips() {
    let change = NpcChange::NoteAdded {
        npc_id: "npc-5".into(),
        note: "Still hiding the ritual knife".into(),
        reason: "The narrator needs long-term NPC memory".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: NpcChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"note_added""#));
}

#[test]
fn npc_visibility_changed_roundtrips() {
    let change = NpcChange::VisibilityChanged {
        npc_id: "npc-6".into(),
        visible_to_player: false,
        reason: "The NPC should remain hidden until introduced".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: NpcChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"visibility_changed""#));
}

// ── FactionChange ─────────────────────────────────────────────────────────────

#[test]
fn faction_standing_changed_roundtrips() {
    let change = FactionChange::StandingChanged {
        faction_id: "guild".into(),
        standing_delta: -5,
        reason: "player betrayed mission".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: FactionChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"standing_changed""#));
}

#[test]
fn faction_goal_revealed_roundtrips() {
    let change = FactionChange::GoalRevealed {
        faction_id: "shadow-council".into(),
        goal: "control the throne".into(),
        reason: "player found the documents".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: FactionChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"goal_revealed""#));
}

#[test]
fn faction_public_note_added_roundtrips() {
    let change = FactionChange::PublicNoteAdded {
        faction_id: "guild".into(),
        note: "Publicly denounced the player".into(),
        reason: "The guild escalated in front of witnesses".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: FactionChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"public_note_added""#));
}

#[test]
fn faction_hidden_note_added_roundtrips() {
    let change = FactionChange::HiddenNoteAdded {
        faction_id: "guild".into(),
        note: "Preparing a covert inquiry".into(),
        reason: "This should remain internal faction memory".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: FactionChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"hidden_note_added""#));
}

// ── ClockChange ───────────────────────────────────────────────────────────────

#[test]
fn clock_advanced_roundtrips() {
    let change = ClockChange::Advanced {
        clock_id: "doom-clock".into(),
        delta: 2,
        reason: "ritual progressed".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: ClockChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"advanced""#));
}

#[test]
fn clock_set_value_roundtrips() {
    let change = ClockChange::SetValue {
        clock_id: "doom-clock".into(),
        value: 5,
        reason: "GM override".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: ClockChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"set_value""#));
}

#[test]
fn clock_visibility_changed_roundtrips() {
    let change = ClockChange::VisibilityChanged {
        clock_id: "doom-clock".into(),
        visible_to_player: false,
        reason: "The countdown should remain hidden".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: ClockChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"visibility_changed""#));
}

// ── QuestChange ───────────────────────────────────────────────────────────────

#[test]
fn quest_started_roundtrips() {
    let change = QuestChange::Started {
        quest_id: "find-artifact".into(),
        reason: "received from elder".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: QuestChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"started""#));
}

#[test]
fn quest_objective_completed_roundtrips() {
    let change = QuestChange::ObjectiveCompleted {
        quest_id: "find-artifact".into(),
        objective_id: "locate-cave".into(),
        reason: "cave discovered".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: QuestChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"objective_completed""#));
}

#[test]
fn quest_completed_roundtrips() {
    let change = QuestChange::Completed {
        quest_id: "find-artifact".into(),
        reason: "artifact retrieved".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: QuestChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"completed""#));
}

#[test]
fn quest_failed_roundtrips() {
    let change = QuestChange::Failed {
        quest_id: "find-artifact".into(),
        reason: "artifact destroyed".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: QuestChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"failed""#));
}

// ── RelationshipChange ────────────────────────────────────────────────────────

#[test]
fn relationship_changed_roundtrips() {
    let change = RelationshipChange::Changed {
        source_id: "npc-1".into(),
        target_id: "npc-2".into(),
        attitude_delta: -10,
        reason: "npc-1 witnessed betrayal".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: RelationshipChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"changed""#));
}

#[test]
fn relationship_note_added_roundtrips() {
    let change = RelationshipChange::NoteAdded {
        source_id: "npc-1".into(),
        target_id: "npc-2".into(),
        note: "Distrust hardened after the betrayal".into(),
        reason: "Long-term relationship memory".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: RelationshipChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"note_added""#));
}

// ── InventoryChange ───────────────────────────────────────────────────────────

#[test]
fn inventory_item_added_roundtrips() {
    let change = InventoryChange::Added {
        item: InventoryItem {
            id: "ritual-knife".into(),
            name: "Ritual Knife".into(),
            description: "Still warm to the touch".into(),
            visible: true,
        },
        reason: "The player took it from the altar".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: InventoryChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"added""#));
}

#[test]
fn inventory_item_removed_roundtrips() {
    let change = InventoryChange::Removed {
        item_id: "ritual-knife".into(),
        reason: "The knife was surrendered to the guild".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: InventoryChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"removed""#));
}

#[test]
fn inventory_item_updated_roundtrips() {
    let change = InventoryChange::Updated {
        item: InventoryItem {
            id: "ritual-knife".into(),
            name: "Ritual Knife".into(),
            description: "Now wrapped in sanctified cloth".into(),
            visible: false,
        },
        reason: "The item was concealed after inspection".into(),
    };
    let json = serde_json::to_string(&change).unwrap();
    let round: InventoryChange = serde_json::from_str(&json).unwrap();
    assert_eq!(change, round);
    assert!(json.contains(r#""type":"updated""#));
}

// ── Scalar enum serde ─────────────────────────────────────────────────────────

#[test]
fn fact_visibility_serializes_correctly() {
    assert_eq!(
        serde_json::to_value(FactVisibility::GmOnly).unwrap(),
        serde_json::json!("gm_only")
    );
    assert_eq!(
        serde_json::to_value(FactVisibility::PlayerKnown).unwrap(),
        serde_json::json!("player_known")
    );
    assert_eq!(
        serde_json::to_value(FactVisibility::NpcKnown).unwrap(),
        serde_json::json!("npc_known")
    );
    assert_eq!(
        serde_json::to_value(FactVisibility::FactionKnown).unwrap(),
        serde_json::json!("faction_known")
    );
}

#[test]
fn turn_mode_serializes_correctly() {
    assert_eq!(
        serde_json::to_value(TurnMode::Action).unwrap(),
        serde_json::json!("action")
    );
    assert_eq!(
        serde_json::to_value(TurnMode::Dialogue).unwrap(),
        serde_json::json!("dialogue")
    );
    assert_eq!(
        serde_json::to_value(TurnMode::Direct).unwrap(),
        serde_json::json!("direct")
    );
    assert_eq!(
        serde_json::to_value(TurnMode::Remember).unwrap(),
        serde_json::json!("remember")
    );
}

#[test]
fn scene_reasoning_style_serializes_correctly() {
    assert_eq!(
        serde_json::to_value(SceneReasoningStyle::TacticalCombat).unwrap(),
        serde_json::json!("tactical_combat")
    );
    assert_eq!(
        serde_json::to_value(SceneReasoningStyle::CharacterDialogue).unwrap(),
        serde_json::json!("character_dialogue")
    );
    assert_eq!(
        serde_json::to_value(SceneReasoningStyle::MysteryInvestigation).unwrap(),
        serde_json::json!("mystery_investigation")
    );
    assert_eq!(
        serde_json::to_value(SceneReasoningStyle::WorldSimulation).unwrap(),
        serde_json::json!("world_simulation")
    );
}

// ── Default value tests ───────────────────────────────────────────────────────

#[test]
fn npc_state_visible_to_player_defaults_to_true() {
    // JSON missing the visible_to_player field
    let json = r#"{"npc_id":"x","status":"active","location_id":null,"attitude_to_player":null,"known_facts":[],"notes":[]}"#;
    let npc: NpcState = serde_json::from_str(json).unwrap();
    assert!(npc.visible_to_player);
}

#[test]
fn clock_state_visible_to_player_defaults_to_true() {
    let json = r#"{"id":"doom-clock","title":"Doom","current":1,"max":6,"consequence":"Ruin"}"#;
    let clock: domain::ClockState = serde_json::from_str(json).unwrap();
    assert!(clock.visible_to_player);
}

#[test]
fn fact_reveal_condition_satisfied_defaults_to_none() {
    let json = r#"{"id":"f1","text":"t","visibility":"gm_only","known_by":[],"source":"scenario","reveal_conditions":[],"related_secret_ids":[]}"#;
    let fact: Fact = serde_json::from_str(json).unwrap();
    assert!(fact.reveal_condition_satisfied.is_none());
}
