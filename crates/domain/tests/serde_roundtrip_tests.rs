use domain::{
    ClockChange, Fact, FactVisibility, FactionChange, InventoryChange, InventoryItem, NpcChange,
    NpcState, NpcStatus, QuestChange, RelationshipChange, SceneReasoningStyle, TurnMode,
};

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
