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
}
