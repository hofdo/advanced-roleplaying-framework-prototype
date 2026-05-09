use domain::{SceneReasoningStyle, WorldState};

pub trait SceneClassifier: Send + Sync {
    fn classify(&self, input: &str, world_state: &WorldState) -> SceneReasoningStyle;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RuleBasedSceneClassifier;

impl SceneClassifier for RuleBasedSceneClassifier {
    fn classify(&self, input: &str, world_state: &WorldState) -> SceneReasoningStyle {
        let lower = input.to_lowercase();

        if world_state.current_scene.as_deref() == Some("combat") {
            return SceneReasoningStyle::TacticalCombat;
        }

        if contains_any(&lower, &["attack", "cast", "strike", "dodge", "shoot"]) {
            return SceneReasoningStyle::TacticalCombat;
        }

        if contains_any(
            &lower,
            &["negotiate", "convince", "threaten", "deal", "bargain"],
        ) {
            return SceneReasoningStyle::PoliticalNegotiation;
        }

        if contains_any(
            &lower,
            &["inspect", "search", "investigate", "clue", "examine"],
        ) {
            return SceneReasoningStyle::MysteryInvestigation;
        }

        if contains_any(
            &lower,
            &["comfort", "grieve", "confess", "reassure", "weep", "embrace"],
        ) {
            return SceneReasoningStyle::EmotionalScene;
        }

        if contains_any(
            &lower,
            &["travel", "journey", "camp", "road", "trail", "explore", "scout"],
        ) {
            return SceneReasoningStyle::TravelExploration;
        }

        if contains_any(
            &lower,
            &["rest", "relax", "shop", "train", "downtime", "recover", "craft"],
        ) {
            return SceneReasoningStyle::Downtime;
        }

        if contains_any(
            &lower,
            &["turn in", "report back", "claim reward", "quest complete", "mission complete"],
        ) {
            return SceneReasoningStyle::QuestResolution;
        }

        if contains_any(&lower, &["class", "stats", "rule", "ability", "level"]) {
            return SceneReasoningStyle::RulesAdjudication;
        }

        SceneReasoningStyle::CharacterDialogue
    }
}

fn contains_any(input: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| input.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::WorldState;
    use uuid::Uuid;

    fn state(scene: Option<&str>) -> WorldState {
        WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 0,
            current_location_id: None,
            current_scene: scene.map(str::to_owned),
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
        }
    }

    #[test]
    fn combat_scene_overrides_input() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("I ask what happened.", &state(Some("combat"))),
            SceneReasoningStyle::TacticalCombat
        );
    }

    #[test]
    fn investigation_keywords_select_mystery_style() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("I inspect the relic for a clue.", &state(None)),
            SceneReasoningStyle::MysteryInvestigation
        );
    }

    #[test]
    fn political_keywords_select_negotiation() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("I want to negotiate a deal.", &state(None)),
            SceneReasoningStyle::PoliticalNegotiation
        );
    }

    #[test]
    fn rules_keywords_select_adjudication() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("What does the rule say about ability checks?", &state(None)),
            SceneReasoningStyle::RulesAdjudication
        );
    }

    #[test]
    fn default_input_selects_character_dialogue() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("Hello there", &state(None)),
            SceneReasoningStyle::CharacterDialogue
        );
    }

    #[test]
    fn combat_keywords_without_scene_override_select_combat() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("I strike the enemy hard.", &state(None)),
            SceneReasoningStyle::TacticalCombat
        );
    }

    #[test]
    fn scene_override_takes_priority_over_input_keywords() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("negotiate deal", &state(Some("combat"))),
            SceneReasoningStyle::TacticalCombat
        );
    }

    #[test]
    fn investigation_keywords_with_combat_scene_still_use_scene() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("I investigate the clue", &state(Some("combat"))),
            SceneReasoningStyle::TacticalCombat
        );
    }

    #[test]
    fn emotional_keywords_select_emotional_scene() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("I comfort her after the funeral.", &state(None)),
            SceneReasoningStyle::EmotionalScene
        );
    }

    #[test]
    fn travel_keywords_select_travel_exploration() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("We travel the road and scout ahead.", &state(None)),
            SceneReasoningStyle::TravelExploration
        );
    }

    #[test]
    fn downtime_keywords_select_downtime() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("I rest, shop, and recover for a day.", &state(None)),
            SceneReasoningStyle::Downtime
        );
    }

    #[test]
    fn quest_resolution_keywords_select_quest_resolution() {
        assert_eq!(
            RuleBasedSceneClassifier.classify("I report back and claim the reward.", &state(None)),
            SceneReasoningStyle::QuestResolution
        );
    }
}
