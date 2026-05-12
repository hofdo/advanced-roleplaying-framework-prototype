//! Built-in sample scenarios. Use `rp scenario create --sample <name>` to seed
//! a fresh session quickly without writing scenario JSON by hand.

use domain::{
    ClockTemplate, Faction, FactionIdentity, Location, Npc, NpcStatus, Quest, RoleIdentity,
    Scenario, ScenarioType, Secret,
};
use uuid::Uuid;

pub fn sample_names() -> Vec<&'static str> {
    vec!["chosen-beyond-goddess"]
}

pub fn build_sample(name: &str) -> anyhow::Result<Scenario> {
    match name {
        "chosen-beyond-goddess" => Ok(chosen_beyond_goddess()),
        other => anyhow::bail!(
            "unknown sample '{other}'; known samples: {}",
            sample_names().join(", ")
        ),
    }
}

fn chosen_beyond_goddess() -> Scenario {
    Scenario {
        id: Uuid::new_v4(),
        title: "Chosen Beyond the Goddess".into(),
        scenario_type: ScenarioType::Adventure,
        setting: "A high fantasy isekai world of sword and magic.".into(),
        tone: "heroic, consequence-driven, high fantasy".into(),
        rules: vec![],
        locations: vec![Location {
            id: "guildhall".into(),
            name: "Guildhall".into(),
            description: "A busy hall filled with examiners and witnesses.".into(),
            visible: true,
        }],
        factions: vec![Faction {
            id: "guild".into(),
            name: "Continental Adventurer Guild".into(),
            description: "Ranks adventurers and monitors dangerous anomalies.".into(),
            faction_identity: FactionIdentity {
                public_goal: "assign quests and protect settlements".into(),
                hidden_goal: Some("monitor calamity-level individuals".into()),
                values: vec!["competence".into(), "contracts".into()],
                fears: vec!["public panic".into()],
                methods: vec!["ranking exams".into()],
            },
            initial_standing: 0,
        }],
        npcs: vec![Npc {
            id: "examiner".into(),
            name: "Guild Examiner".into(),
            description: "A veteran examiner with a careful eye.".into(),
            role_identity: RoleIdentity {
                core_emotion: "alert".into(),
                motivation: "protect civilians while evaluating the player".into(),
                worldview: "power demands accountability".into(),
                fear: Some("uncontrolled magical catastrophe".into()),
                desire: None,
                speech_style: "measured and formal".into(),
                boundaries: vec!["will not ignore public danger".into()],
                values: vec!["order".into()],
            },
            stats: None,
            initial_status: NpcStatus::Active,
        }],
        quests: vec![Quest {
            id: "register".into(),
            title: "Register at the Guild".into(),
            description: "Complete the registration process.".into(),
            objectives: vec![],
            visible: true,
        }],
        secrets: vec![Secret {
            id: "void-mark".into(),
            text: "The player's soul-mark was not created by the goddess.".into(),
            reveal_conditions: vec!["a divine relic reacts to the mark".into()],
        }],
        clocks: vec![ClockTemplate {
            id: "fame".into(),
            title: "The player's fame spreads".into(),
            current: 1,
            max: 6,
            consequence: "Major factions start treating the player as a strategic threat.".into(),
        }],
    }
}
