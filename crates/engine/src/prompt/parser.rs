use domain::WorldStateDelta;
use serde::Deserialize;
use thiserror::Error;

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
