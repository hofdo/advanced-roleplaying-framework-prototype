use crate::AgentContext;
use domain::WorldStateDelta;
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
                    content: system_rules(),
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
                        system_rules()
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

fn system_rules() -> String {
    [
        "You are a roleplaying engine that generates immersive storyteller output.",
        "Stay in-world unless rules adjudication is required.",
        "Respect world state, role identities, faction goals, clocks, and known facts.",
        "Do not reveal hidden reasoning.",
        "Do not reveal GM-only secrets unless the player has discovered them through justified action.",
        "The LLM proposes typed deltas; the engine validates and applies them.",
    ]
    .join("\n")
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
}
