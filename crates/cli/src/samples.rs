//! Built-in sample scenarios. Use `rp scenario create --sample <name>` to seed
//! a fresh session quickly without writing scenario JSON by hand.

use anyhow::{Context, Result};
use domain::Scenario;
use uuid::Uuid;

const SAMPLE_REGISTRY: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/sample_registry.rs"));

pub fn sample_names() -> Vec<&'static str> {
    SAMPLE_REGISTRY.iter().map(|(name, _)| *name).collect()
}

pub fn build_sample(name: &str) -> Result<Scenario> {
    let (_, raw) = SAMPLE_REGISTRY
        .iter()
        .find(|(sample_name, _)| *sample_name == name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown sample '{name}'; known samples: {}",
                sample_names().join(", ")
            )
        })?;

    let mut scenario: Scenario =
        serde_json::from_str(raw).with_context(|| format!("parsing built-in sample {name}"))?;
    scenario.id = Uuid::new_v4();
    domain::validate_scenario(&scenario)
        .with_context(|| format!("validating built-in sample {name}"))?;
    Ok(scenario)
}

pub fn template_json() -> &'static str {
    include_str!("../scenarios/templates/scenario.template.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_names_are_generated_from_all_builtin_sample_files() {
        assert_eq!(
            sample_names(),
            vec![
                "ashfall-murder",
                "bride-of-the-iron-archduke",
                "chosen-beyond-goddess",
                "glass-senate-crisis"
            ]
        );
    }

    #[test]
    fn all_builtin_samples_deserialize_and_validate() {
        for name in sample_names() {
            let scenario = build_sample(name).expect("sample should build");
            domain::validate_scenario(&scenario).expect("sample should validate");
        }
    }

    #[test]
    fn builtin_samples_get_fresh_ids_per_build() {
        let first = build_sample("chosen-beyond-goddess").expect("sample should build");
        let second = build_sample("chosen-beyond-goddess").expect("sample should build");

        assert_ne!(first.id, second.id);
    }

    #[test]
    fn unknown_sample_error_lists_all_generated_names() {
        let err = build_sample("missing-sample").expect_err("sample should be unknown");
        let message = err.to_string();

        assert!(message.contains("ashfall-murder"));
        assert!(message.contains("bride-of-the-iron-archduke"));
        assert!(message.contains("chosen-beyond-goddess"));
        assert!(message.contains("glass-senate-crisis"));
    }

    #[test]
    fn scenario_authoring_template_deserializes_and_validates() {
        let template = include_str!("../scenarios/templates/scenario.template.json");
        let scenario: Scenario = serde_json::from_str(template).expect("template should parse");

        domain::validate_scenario(&scenario).expect("template should validate");
    }

    #[test]
    fn bride_of_the_iron_archduke_opens_with_marta() {
        let scenario = build_sample("bride-of-the-iron-archduke").expect("sample should build");

        assert_eq!(
            scenario.npcs.first().map(|npc| npc.id.as_str()),
            Some("steward-marta")
        );
    }

    #[test]
    fn bride_of_the_iron_archduke_tracks_core_pressure_clocks() {
        let scenario = build_sample("bride-of-the-iron-archduke").expect("sample should build");
        let clock_ids = scenario
            .clocks
            .iter()
            .map(|clock| clock.id.as_str())
            .collect::<Vec<_>>();

        assert!(clock_ids.contains(&"wedding-approaches"));
        assert!(clock_ids.contains(&"imperial-pressure"));
        assert!(clock_ids.contains(&"ashen-court-sabotage"));
    }

    #[test]
    fn bride_of_the_iron_archduke_has_romance_boundary_rule() {
        let scenario = build_sample("bride-of-the-iron-archduke").expect("sample should build");

        assert!(
            scenario
                .rules
                .iter()
                .any(|rule| rule.contains("Romance should emerge"))
        );
    }

    #[test]
    fn bride_of_the_iron_archduke_keeps_key_reputation_secrets() {
        let scenario = build_sample("bride-of-the-iron-archduke").expect("sample should build");
        let secret_ids = scenario
            .secrets
            .iter()
            .map(|secret| secret.id.as_str())
            .collect::<Vec<_>>();

        assert!(secret_ids.contains(&"ashen-court-forged-rumors"));
        assert!(secret_ids.contains(&"severin-protects-orphan-house"));
    }
}
