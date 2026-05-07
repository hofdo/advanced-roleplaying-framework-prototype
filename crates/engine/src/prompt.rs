use crate::AgentContext;
use domain::{TurnMode, WorldStateDelta};
use providers::{LlmMessage, LlmMessageRole, LlmRequest};
use serde::Deserialize;
use thiserror::Error;

pub const PROMPT_TEMPLATE_VERSION: &str = "roleplaying-engine-v1";

pub trait PromptBuilder: Send + Sync {
    fn build_non_streaming_prompt(&self, context: &AgentContext, player_input: &str) -> LlmRequest;
    fn build_streaming_prompt(&self, context: &AgentContext, player_input: &str) -> LlmRequest;
    fn build_delta_extraction_prompt(
        &self,
        context: &AgentContext,
        player_input: &str,
        visible_response: &str,
    ) -> LlmRequest;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicPromptBuilder;

impl PromptBuilder for BasicPromptBuilder {
    fn build_non_streaming_prompt(&self, context: &AgentContext, player_input: &str) -> LlmRequest {
        LlmRequest {
            messages: vec![
                LlmMessage {
                    role: LlmMessageRole::System,
                    content: system_rules(context.mode),
                },
                LlmMessage {
                    role: LlmMessageRole::User,
                    content: format!(
                        "{}\n\nPLAYER INPUT:\n{}\n\nOUTPUT CONTRACT:\nReturn strict JSON with keys player_response and world_state_delta. The delta may only use typed change arrays.",
                        render_context(context),
                        player_input
                    ),
                },
            ],
            temperature: Some(0.7),
            max_tokens: None,
            json_mode: true,
        }
    }

    fn build_streaming_prompt(&self, context: &AgentContext, player_input: &str) -> LlmRequest {
        LlmRequest {
            messages: vec![
                LlmMessage {
                    role: LlmMessageRole::System,
                    content: format!(
                        "{}\nStream only player-visible narration. Do not emit JSON.",
                        system_rules(context.mode)
                    ),
                },
                LlmMessage {
                    role: LlmMessageRole::User,
                    content: format!(
                        "{}\n\nPLAYER INPUT:\n{}",
                        render_context(context),
                        player_input
                    ),
                },
            ],
            temperature: Some(0.8),
            max_tokens: None,
            json_mode: false,
        }
    }

    fn build_delta_extraction_prompt(
        &self,
        context: &AgentContext,
        player_input: &str,
        visible_response: &str,
    ) -> LlmRequest {
        LlmRequest {
            messages: vec![
                LlmMessage {
                    role: LlmMessageRole::System,
                    content: "Extract only a typed WorldStateDelta as strict JSON. Do not narrate."
                        .into(),
                },
                LlmMessage {
                    role: LlmMessageRole::User,
                    content: format!(
                        "{}\n\nPLAYER INPUT:\n{}\n\nVISIBLE RESPONSE:\n{}\n\nReturn only world_state_delta JSON.",
                        render_context(context),
                        player_input,
                        visible_response
                    ),
                },
            ],
            temperature: Some(0.2),
            max_tokens: None,
            json_mode: true,
        }
    }
}

fn system_rules(mode: Option<TurnMode>) -> String {
    let base = [
        "You are a roleplaying engine that generates immersive storyteller output.",
        "Stay in-world unless rules adjudication is required.",
        "Respect world state, role identities, faction goals, clocks, and known facts.",
        "Do not reveal hidden reasoning.",
        "Do not reveal GM-only secrets unless the player has discovered them through justified action.",
        "The LLM proposes typed deltas; the engine validates and applies them.",
    ]
    .join("\n");

    let preamble = match mode {
        Some(TurnMode::Action) => {
            Some("The player is performing an in-world action. Narrate the outcome.")
        }
        Some(TurnMode::Direct) => {
            Some("The player is asking an out-of-character question. Answer as GM directly and clearly. Do not stay in character.")
        }
        Some(TurnMode::Remember) => {
            Some("The player is providing a memory or fact correction. Acknowledge it, update your understanding, and confirm what changed.")
        }
        // Dialogue and None: no preamble — character dialogue is the default
        Some(TurnMode::Dialogue) | None => None,
    };

    match preamble {
        Some(text) => format!("{text}\n{base}"),
        None => base,
    }
}

fn render_context(context: &AgentContext) -> String {
    format!(
        "SCENARIO: {}\nSETTING: {}\nSCENE TYPE: {:?}\nPRIORITIZE: {}\nAVOID: {}\nACTIVE ROLE: {}\nPLAYER-KNOWN FACTS: {}\nRELEVANT GM-ONLY FACTS (labeled, do not reveal unless justified): {}\nRECENT SUMMARY: {}",
        context.scenario_title,
        context.setting_summary,
        context.scene_directive.style,
        context.scene_directive.priorities.join("; "),
        context.scene_directive.avoid.join("; "),
        context
            .active_role
            .active_role_name
            .as_deref()
            .unwrap_or("Narrator/GM"),
        context
            .player_known_facts
            .iter()
            .map(|fact| fact.text.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        context
            .gm_only_facts
            .iter()
            .map(|fact| format!(
                "{} (reveal: {})",
                fact.text,
                fact.reveal_conditions.join("; ")
            ))
            .collect::<Vec<_>>()
            .join(" | "),
        context.recent_summary.as_deref().unwrap_or("")
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerTurnModelOutput {
    pub player_response: String,
    pub world_state_delta: WorldStateDelta,
}

pub trait ResponseParser: Send + Sync {
    fn parse_turn_output(&self, raw: &str) -> Result<PlayerTurnModelOutput, ParseError>;
    fn parse_delta_output(&self, raw: &str) -> Result<WorldStateDelta, ParseError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct JsonResponseParser;

impl ResponseParser for JsonResponseParser {
    fn parse_turn_output(&self, raw: &str) -> Result<PlayerTurnModelOutput, ParseError> {
        match serde_json::from_str(raw) {
            Ok(output) => Ok(output),
            Err(_) => serde_json::from_str(extract_json_object(raw)?)
                .map_err(|error| ParseError::Malformed(error.to_string())),
        }
    }

    fn parse_delta_output(&self, raw: &str) -> Result<WorldStateDelta, ParseError> {
        match serde_json::from_str(raw) {
            Ok(delta) => Ok(delta),
            Err(_) => serde_json::from_str(extract_json_object(raw)?)
                .map_err(|error| ParseError::Malformed(error.to_string())),
        }
    }
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("malformed model output: {0}")]
    Malformed(String),
}

fn extract_json_object(raw: &str) -> Result<&str, ParseError> {
    let start = raw
        .find('{')
        .ok_or_else(|| ParseError::Malformed("missing JSON object start".into()))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| ParseError::Malformed("missing JSON object end".into()))?;
    Ok(&raw[start..=end])
}

/// Build a repair prompt that asks the LLM to return corrected JSON for the
/// `WorldStateDelta` schema, given the malformed output it previously returned.
///
/// The repair call goes directly to `provider.generate()`; it must NOT go
/// through the full turn pipeline.
pub fn repair_prompt(raw_output: &str) -> String {
    format!(
        "The following output was malformed JSON. \
         Return only valid JSON matching the WorldStateDelta schema. \
         Do not include any explanation or markdown.\n\
         \n\
         Schema fields: facts_to_add, npc_changes, faction_changes, \
         quest_changes, clock_changes, relationship_changes, \
         location_change, event_log_entries\n\
         \n\
         Malformed output:\n{raw_output}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_extracts_turn_json_from_wrapped_text() {
        let raw = r#"prefix {"player_response":"Hi","world_state_delta":{"facts_to_add":[],"npc_changes":[],"faction_changes":[],"quest_changes":[],"clock_changes":[],"relationship_changes":[],"location_change":null,"event_log_entries":[]}} suffix"#;

        let parsed = JsonResponseParser.parse_turn_output(raw).expect("parsed");

        assert_eq!(parsed.player_response, "Hi");
        assert!(parsed.world_state_delta.event_log_entries.is_empty());
    }

    #[test]
    fn repair_prompt_contains_schema_fields_and_raw_output() {
        let prompt = repair_prompt("{ bad json }");
        assert!(prompt.contains("facts_to_add"));
        assert!(prompt.contains("npc_changes"));
        assert!(prompt.contains("{ bad json }"));
    }

    // ---- TurnMode prompt shaping tests ----

    fn minimal_context(mode: Option<TurnMode>) -> crate::AgentContext {
        use crate::{
            FactionContext, MessageContext, NpcContext, ReasoningStyleDirective,
            RoleActivationContext,
        };
        use domain::SceneReasoningStyle;
        crate::AgentContext {
            scenario_title: "Test".into(),
            setting_summary: "Test setting".into(),
            current_location: None,
            active_role: RoleActivationContext {
                active_role_name: None,
                emotion_now: None,
                motivation_now: None,
                knowledge_boundaries: vec![],
                forbidden_moves: vec![],
                speech_constraints: vec![],
            },
            scene_directive: ReasoningStyleDirective {
                style: SceneReasoningStyle::CharacterDialogue,
                priorities: vec![],
                avoid: vec![],
                visible_response_shape: "narration".into(),
            },
            relevant_npcs: vec![],
            relevant_factions: vec![],
            active_quests: vec![],
            active_clocks: vec![],
            player_known_facts: vec![],
            gm_only_facts: vec![],
            recent_summary: None,
            recent_messages: vec![],
            rules: vec![],
            mode,
        }
    }

    fn system_message_content(request: &providers::LlmRequest) -> &str {
        request
            .messages
            .iter()
            .find(|m| m.role == providers::LlmMessageRole::System)
            .map(|m| m.content.as_str())
            .unwrap_or("")
    }

    #[test]
    fn direct_mode_sets_gm_system_prompt() {
        let ctx = minimal_context(Some(TurnMode::Direct));
        let request = BasicPromptBuilder.build_non_streaming_prompt(&ctx, "What is the rule?");
        let system = system_message_content(&request);
        assert!(
            system.contains("out-of-character") || system.contains("GM directly"),
            "expected GM direct answer preamble, got: {system}"
        );
    }

    #[test]
    fn remember_mode_sets_fact_correction_prompt() {
        let ctx = minimal_context(Some(TurnMode::Remember));
        let request = BasicPromptBuilder.build_non_streaming_prompt(&ctx, "Remember that...");
        let system = system_message_content(&request);
        assert!(
            system.contains("fact correction") || system.contains("memory"),
            "expected memory/fact correction preamble, got: {system}"
        );
    }

    #[test]
    fn dialogue_mode_uses_default_prompt() {
        let ctx_dialogue = minimal_context(Some(TurnMode::Dialogue));
        let ctx_none = minimal_context(None);
        let req_d = BasicPromptBuilder.build_non_streaming_prompt(&ctx_dialogue, "Hello.");
        let req_n = BasicPromptBuilder.build_non_streaming_prompt(&ctx_none, "Hello.");
        let sys_d = system_message_content(&req_d);
        let sys_n = system_message_content(&req_n);
        // Neither should contain a mode-specific preamble
        assert!(
            !sys_d.contains("out-of-character") && !sys_d.contains("fact correction"),
            "Dialogue mode should not add special preamble, got: {sys_d}"
        );
        assert!(
            !sys_n.contains("out-of-character") && !sys_n.contains("fact correction"),
            "None mode should not add special preamble, got: {sys_n}"
        );
        // Both should produce the same system prompt
        assert_eq!(sys_d, sys_n);
    }
}
