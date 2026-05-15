use anyhow::{Context, Result};
use domain::Scenario;

pub fn read_scenario_file(path: &str) -> Result<Scenario> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {path}"))?;
    parse_scenario_json(&bytes, path)
}

pub fn parse_scenario_json(bytes: &[u8], source: &str) -> Result<Scenario> {
    let scenario: Scenario =
        serde_json::from_slice(bytes).with_context(|| format!("parsing scenario {source}"))?;
    domain::validate_scenario(&scenario)
        .with_context(|| format!("validating scenario {source}"))?;
    Ok(scenario)
}

pub fn scenario_summary(scenario: &Scenario) -> String {
    let opening_location = scenario
        .locations
        .first()
        .map(|location| format!("{} ({})", location.name, location.id))
        .unwrap_or_else(|| "(none)".into());
    let opening_speaker = scenario
        .npcs
        .first()
        .map(|npc| format!("{} ({})", npc.name, npc.id))
        .unwrap_or_else(|| "(none)".into());
    let hidden_npc_count = scenario
        .npcs
        .iter()
        .filter(|npc| !npc.initial_visible_to_player)
        .count();

    format!(
        "\
Title: {title}
Tone: {tone}
Opening location: {opening_location}
Opening speaker: {opening_speaker}
Location count: {location_count}
NPC count: {npc_count}
Hidden NPC count: {hidden_npc_count}
Faction count: {faction_count}
Quest count: {quest_count}
Secret count: {secret_count}
Clock count: {clock_count}",
        title = scenario.title,
        tone = scenario.tone,
        opening_location = opening_location,
        opening_speaker = opening_speaker,
        location_count = scenario.locations.len(),
        npc_count = scenario.npcs.len(),
        hidden_npc_count = hidden_npc_count,
        faction_count = scenario.factions.len(),
        quest_count = scenario.quests.len(),
        secret_count = scenario.secrets.len(),
        clock_count = scenario.clocks.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn parse_scenario_json_preserves_file_id() {
        let id = Uuid::new_v4();
        let raw = format!(
            r#"{{
                "id": "{id}",
                "title": "Valid File Scenario",
                "scenario_type": "adventure",
                "setting": "Test setting",
                "tone": "test",
                "rules": [],
                "locations": [],
                "factions": [],
                "npcs": [],
                "quests": [],
                "secrets": [],
                "clocks": []
            }}"#
        );

        let scenario = parse_scenario_json(raw.as_bytes(), "inline").expect("scenario parses");

        assert_eq!(scenario.id, id);
    }

    #[test]
    fn parse_scenario_json_rejects_invalid_domain_shape() {
        let raw = r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "title": "Invalid File Scenario",
            "scenario_type": "adventure",
            "setting": "Test setting",
            "tone": "test",
            "rules": [],
            "locations": [
                {
                    "id": "same",
                    "name": "One",
                    "description": "One",
                    "visible": true
                },
                {
                    "id": "same",
                    "name": "Two",
                    "description": "Two",
                    "visible": true
                }
            ],
            "factions": [],
            "npcs": [],
            "quests": [],
            "secrets": [],
            "clocks": []
        }"#;

        let err = parse_scenario_json(raw.as_bytes(), "inline")
            .expect_err("duplicate IDs should be rejected");

        assert!(err.to_string().contains("validating scenario inline"));
    }

    #[test]
    fn scenario_summary_names_opening_entities() {
        let scenario = parse_scenario_json(
            include_bytes!("../scenarios/templates/scenario.template.json"),
            "template",
        )
        .expect("template parses");

        let summary = scenario_summary(&scenario);

        assert!(summary.contains("Opening location:"));
        assert!(summary.contains("Opening speaker:"));
        assert!(summary.contains("Clock count:"));
    }
}
