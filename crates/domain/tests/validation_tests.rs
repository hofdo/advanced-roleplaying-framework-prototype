use domain::{
    ClockTemplate, EntityKey, Fact, FactSource, FactVisibility, Faction, FactionIdentity, Location,
    Npc, NpcStatus, PlayerCharacterState, Quest, RevealCondition, RoleIdentity, Scenario,
    ScenarioType, Secret, WorldState, validate_npc_status_transition, validate_scenario,
};
use uuid::Uuid;

fn role_identity() -> RoleIdentity {
    RoleIdentity {
        core_emotion: "protective".into(),
        motivation: "guide the player".into(),
        worldview: "power requires responsibility".into(),
        fear: Some("the player becomes a calamity".into()),
        desire: None,
        speech_style: "warm and restrained".into(),
        boundaries: vec!["cannot reveal the full truth early".into()],
        values: vec!["patience".into()],
    }
}

fn faction_identity() -> FactionIdentity {
    FactionIdentity {
        public_goal: "protect settlements".into(),
        hidden_goal: Some("monitor calamity-level people".into()),
        values: vec!["competence".into()],
        fears: vec!["public panic".into()],
        methods: vec!["senior observers".into()],
    }
}

fn scenario() -> Scenario {
    Scenario {
        id: Uuid::new_v4(),
        title: "Chosen Beyond the Goddess".into(),
        scenario_type: ScenarioType::Adventure,
        setting: "high fantasy isekai".into(),
        tone: "heroic, consequence-driven".into(),
        rules: vec![],
        locations: vec![Location {
            id: "hall-of-the-goddess".into(),
            name: "Hall of the Goddess".into(),
            description: "A white marble hall outside mortal time.".into(),
            visible: true,
        }],
        factions: vec![Faction {
            id: "adventurer-guild".into(),
            name: "Continental Adventurer Guild".into(),
            description: "Controls quest access.".into(),
            faction_identity: faction_identity(),
            initial_standing: 0,
        }],
        npcs: vec![Npc {
            id: "seraphyne".into(),
            name: "Seraphyne".into(),
            description: "Goddess who guides summoned souls.".into(),
            role_identity: role_identity(),
            stats: None,
            initial_status: NpcStatus::Active,
            initial_location_id: None,
            initial_visible_to_player: true,
        }],
        quests: vec![Quest {
            id: "choose-class".into(),
            title: "Choose a Class".into(),
            description: "Select a starting class.".into(),
            objectives: vec![],
            visible: true,
        }],
        secrets: vec![Secret {
            id: "void-mark-source".into(),
            text: "The mark was not created by the goddess.".into(),
            reveal_conditions: vec![RevealCondition {
                id: "divine-relic-reacts".into(),
                description: "A divine relic reacts.".into(),
            }],
        }],
        clocks: vec![ClockTemplate {
            id: "player-fame-spreads".into(),
            title: "The player's fame spreads".into(),
            current: 1,
            max: 6,
            consequence: "Major factions notice the player.".into(),
        }],
    }
}

#[test]
fn scenario_validation_rejects_duplicate_entity_ids() {
    let mut scenario = scenario();
    scenario.npcs.push(Npc {
        id: "seraphyne".into(),
        name: "False Seraphyne".into(),
        description: "Duplicate id.".into(),
        role_identity: role_identity(),
        stats: None,
        initial_status: NpcStatus::Active,
        initial_location_id: None,
        initial_visible_to_player: true,
    });

    let err = validate_scenario(&scenario).expect_err("duplicate ID must be rejected");

    assert!(err.to_string().contains("duplicate npc id"));
}

#[test]
fn scenario_validation_rejects_clock_values_above_max() {
    let mut scenario = scenario();
    scenario.clocks[0].current = 7;

    let err = validate_scenario(&scenario).expect_err("clock above max must be rejected");

    assert!(
        err.to_string()
            .contains("clock player-fame-spreads current exceeds max")
    );
}

#[test]
fn scenario_validation_rejects_unknown_initial_npc_location() {
    let mut scenario = scenario();
    scenario.npcs[0].initial_location_id = Some("missing-location".into());

    let err = validate_scenario(&scenario).expect_err("unknown NPC location must be rejected");

    assert!(
        err.to_string()
            .contains("unknown location id missing-location")
    );
}

#[test]
fn dead_npc_cannot_become_active_without_revival_event() {
    let err = validate_npc_status_transition(NpcStatus::Dead, NpcStatus::Active, false)
        .expect_err("dead to active without revival must be rejected");

    assert!(err.to_string().contains("revival event"));
}

#[test]
fn world_state_authoritative_facts_can_distinguish_secret_visibility() {
    let state = WorldState {
        session_id: Uuid::new_v4(),
        scenario_id: Uuid::new_v4(),
        version: 0,
        current_location_id: Some(EntityKey::from("hall-of-the-goddess")),
        current_scene: Some("class_selection".into()),
        active_speaker_id: Some("seraphyne".into()),
        facts: vec![
            Fact {
                id: "public-welcome".into(),
                text: "Seraphyne welcomed the player.".into(),
                visibility: FactVisibility::PlayerKnown,
                known_by: vec![],
                source: FactSource::Scenario,
                reveal_conditions: vec![],
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            },
            Fact {
                id: "void-mark-source".into(),
                text: "The mark was not created by the goddess.".into(),
                visibility: FactVisibility::GmOnly,
                known_by: vec![],
                source: FactSource::Scenario,
                reveal_conditions: vec![RevealCondition {
                    id: "divine-relic-reacts".into(),
                    description: "A divine relic reacts.".into(),
                }],
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            },
        ],
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

    assert_eq!(
        state
            .facts
            .iter()
            .filter(|fact| fact.visibility == FactVisibility::PlayerKnown)
            .count(),
        1
    );
    assert_eq!(
        state
            .facts
            .iter()
            .filter(|fact| fact.visibility == FactVisibility::GmOnly)
            .count(),
        1
    );
}
