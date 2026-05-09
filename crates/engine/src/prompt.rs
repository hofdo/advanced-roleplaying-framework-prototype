use crate::AgentContext;
use domain::{TurnMode, WorldStateDelta};
use providers::{LlmMessage, LlmMessageRole, LlmRequest};
use serde::Deserialize;
use std::collections::HashSet;
use thiserror::Error;

pub const PROMPT_TEMPLATE_VERSION: &str = "roleplaying-engine-v2";

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
                        "{}\n\nOUTPUT CONTRACT:\nReturn strict JSON with keys player_response and world_state_delta. The delta may only use typed change arrays.",
                        render_context(context, player_input),
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
                    content: render_context(context, player_input),
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
                        "{}\n\nVISIBLE RESPONSE:\n{}\n\nOUTPUT CONTRACT:\nReturn only world_state_delta JSON.",
                        render_context(context, player_input),
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

fn render_context(context: &AgentContext, player_input: &str) -> String {
    [
        render_section(
            "SCENARIO AND SETTING",
            &[
                format!("Scenario: {}", context.scenario_title),
                format!("Setting: {}", context.setting_summary),
                format!("Rules: {}", join_or_none(&context.rules)),
            ],
        ),
        render_section(
            "SCENE STYLE DIRECTIVE",
            &[
                format!("Style: {:?}", context.scene_directive.style),
                format!(
                    "Priorities: {}",
                    join_or_none(&context.scene_directive.priorities)
                ),
                format!("Avoid: {}", join_or_none(&context.scene_directive.avoid)),
                format!(
                    "Visible response shape: {}",
                    context.scene_directive.visible_response_shape
                ),
            ],
        ),
        render_section(
            "ACTIVE ROLE ACTIVATION",
            &[
                format!(
                    "Active role: {}",
                    context
                        .active_role
                        .active_role_name
                        .as_deref()
                        .unwrap_or("Narrator/GM")
                ),
                format!(
                    "Emotion now: {}",
                    context.active_role.emotion_now.as_deref().unwrap_or("none")
                ),
                format!(
                    "Motivation now: {}",
                    context
                        .active_role
                        .motivation_now
                        .as_deref()
                        .unwrap_or("none")
                ),
                format!(
                    "Knowledge boundaries: {}",
                    join_or_none(&context.active_role.knowledge_boundaries)
                ),
                format!(
                    "Forbidden moves: {}",
                    join_or_none(&context.active_role.forbidden_moves)
                ),
                format!(
                    "Speech constraints: {}",
                    join_or_none(&context.active_role.speech_constraints)
                ),
            ],
        ),
        render_section(
            "CURRENT WORLD STATE",
            &[
                format!(
                    "Current location: {}",
                    context
                        .current_location
                        .as_ref()
                        .map(|location| format!("{} — {}", location.name, location.description))
                        .unwrap_or_else(|| "none".into())
                ),
                format!(
                    "Relevant NPCs: {}",
                    if context.relevant_npcs.is_empty() {
                        "none".into()
                    } else {
                        context
                            .relevant_npcs
                            .iter()
                            .map(|npc| {
                                format!(
                                    "{} ({:?}; attitude: {})",
                                    npc.npc.name,
                                    npc.status,
                                    npc.attitude_to_player.as_deref().unwrap_or("none")
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" | ")
                    }
                ),
                format!(
                    "Relevant factions: {}",
                    if context.relevant_factions.is_empty() {
                        "none".into()
                    } else {
                        context
                            .relevant_factions
                            .iter()
                            .map(|faction| {
                                let standing = faction
                                    .state
                                    .as_ref()
                                    .map(|state| state.standing.to_string())
                                    .unwrap_or_else(|| "none".into());
                                format!(
                                    "{} (standing: {}; public goal: {})",
                                    faction.faction.name,
                                    standing,
                                    faction.faction.faction_identity.public_goal
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" | ")
                    }
                ),
                format!(
                    "Active quests: {}",
                    if context.active_quests.is_empty() {
                        "none".into()
                    } else {
                        context
                            .active_quests
                            .iter()
                            .map(|quest| format!("{} ({:?})", quest.quest_id, quest.status))
                            .collect::<Vec<_>>()
                            .join(" | ")
                    }
                ),
                format!(
                    "Active clocks: {}",
                    if context.active_clocks.is_empty() {
                        "none".into()
                    } else {
                        context
                            .active_clocks
                            .iter()
                            .map(|clock| {
                                format!(
                                    "{} ({}/{}; consequence: {})",
                                    clock.title, clock.current, clock.max, clock.consequence
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" | ")
                    }
                ),
            ],
        ),
        render_section(
            "RELEVANT FACTS",
            &[
                format!(
                    "Player-known facts: {}",
                    if context.player_known_facts.is_empty() {
                        "none".into()
                    } else {
                        context
                            .player_known_facts
                            .iter()
                            .map(|fact| fact.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" | ")
                    }
                ),
                format!(
                    "GM-only facts (do not reveal unless justified): {}",
                    render_gm_only_facts(context, player_input)
                ),
            ],
        ),
        render_single_value_section(
            "RECENT SUMMARY",
            context.recent_summary.as_deref().unwrap_or("none"),
        ),
        render_section(
            "RECENT MESSAGES",
            &[if context.recent_messages.is_empty() {
                "none".into()
            } else {
                context
                    .recent_messages
                    .iter()
                    .map(|message| format!("{}: {}", message.role, message.content))
                    .collect::<Vec<_>>()
                    .join(" | ")
            }],
        ),
        render_single_value_section("PLAYER INPUT", player_input),
    ]
    .join("\n\n")
}

fn render_gm_only_facts(context: &AgentContext, player_input: &str) -> String {
    let relevant = relevant_gm_only_facts(context, player_input);
    if relevant.is_empty() {
        "none".into()
    } else {
        relevant
            .iter()
            .map(|fact| {
                let reveal_conditions = if fact.reveal_conditions.is_empty() {
                    "none".into()
                } else {
                    fact.reveal_conditions.join("; ")
                };
                format!("{} (reveal conditions: {reveal_conditions})", fact.text)
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

fn relevant_gm_only_facts<'a>(
    context: &'a AgentContext,
    player_input: &str,
) -> Vec<&'a domain::Fact> {
    let location_name = context
        .current_location
        .as_ref()
        .map(|location| location.name.as_str())
        .unwrap_or("");
    let active_role_name = context.active_role.active_role_name.as_deref().unwrap_or("");
    let quest_text = context
        .active_quests
        .iter()
        .map(|quest| quest.quest_id.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let clock_text = context
        .active_clocks
        .iter()
        .map(|clock| clock.title.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let cues = tokenize(&format!(
        "{player_input} {location_name} {active_role_name} {quest_text} {clock_text}"
    ));

    context
        .gm_only_facts
        .iter()
        .filter(|fact| {
            let fact_text = format!("{} {}", fact.text, fact.reveal_conditions.join(" "));
            let fact_tokens = tokenize(&fact_text);
            !fact_tokens.is_disjoint(&cues)
        })
        .collect()
}

fn tokenize(input: &str) -> HashSet<String> {
    input
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|token| token.to_ascii_lowercase())
        .filter(|token| token.len() >= 3 && !is_noise_token(token))
        .collect()
}

fn is_noise_token(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "with"
            | "that"
            | "this"
            | "from"
            | "into"
            | "stop"
            | "clock"
            | "only"
            | "during"
    )
}

fn render_section(title: &str, lines: &[String]) -> String {
    format!("{title}:\n{}", lines.join("\n"))
}

fn render_single_value_section(title: &str, value: &str) -> String {
    format!("{title}:\n{value}")
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join("; ")
    }
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
        use crate::{ReasoningStyleDirective, RoleActivationContext};
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

    fn user_message_content(request: &providers::LlmRequest) -> &str {
        request
            .messages
            .iter()
            .find(|m| m.role == providers::LlmMessageRole::User)
            .map(|m| m.content.as_str())
            .unwrap_or("")
    }

    fn rich_context(mode: Option<TurnMode>) -> crate::AgentContext {
        use crate::{
            FactionContext, MessageContext, NpcContext, ReasoningStyleDirective,
            RoleActivationContext,
        };
        use domain::{
            ClockState, Fact, FactSource, FactVisibility, Faction, FactionIdentity, FactionState,
            Location, Npc, NpcStatus, QuestState, QuestStatus, RoleIdentity,
            SceneReasoningStyle,
        };

        crate::AgentContext {
            scenario_title: "Aurethia".into(),
            setting_summary: "High fantasy city under magical strain".into(),
            current_location: Some(Location {
                id: "guildhall".into(),
                name: "Guildhall".into(),
                description: "A vaulted hall humming with mana.".into(),
                visible: true,
            }),
            active_role: RoleActivationContext {
                active_role_name: Some("Seraphyne".into()),
                emotion_now: Some("worried".into()),
                motivation_now: Some("guide carefully".into()),
                knowledge_boundaries: vec!["Does not know the void source".into()],
                forbidden_moves: vec!["Do not reveal GM-only secrets early".into()],
                speech_constraints: vec!["Solemn and precise".into()],
            },
            scene_directive: ReasoningStyleDirective {
                style: SceneReasoningStyle::PoliticalNegotiation,
                priorities: vec!["track leverage".into(), "show visible consequence".into()],
                avoid: vec!["generic exposition".into()],
                visible_response_shape: "immersive dialogue plus visible social consequence"
                    .into(),
            },
            relevant_npcs: vec![NpcContext {
                npc: Npc {
                    id: "seraphyne".into(),
                    name: "Seraphyne".into(),
                    description: "A solemn goddess.".into(),
                    role_identity: RoleIdentity {
                        core_emotion: "worried".into(),
                        motivation: "guide carefully".into(),
                        worldview: "power requires responsibility".into(),
                        fear: None,
                        desire: None,
                        speech_style: "solemn".into(),
                        boundaries: vec!["Avoids speaking of hidden ruin".into()],
                        values: vec!["mercy".into()],
                    },
                    stats: None,
                    initial_status: NpcStatus::Active,
                },
                status: NpcStatus::Active,
                attitude_to_player: Some("cautious warmth".into()),
            }],
            relevant_factions: vec![FactionContext {
                faction: Faction {
                    id: "guild".into(),
                    name: "Adventurers Guild".into(),
                    description: "The city's licensed adventurers.".into(),
                    faction_identity: FactionIdentity {
                        public_goal: "Keep order".into(),
                        hidden_goal: Some("Contain the mana anomaly".into()),
                        values: vec!["discipline".into()],
                        fears: vec!["public panic".into()],
                        methods: vec!["quiet pressure".into()],
                    },
                    initial_standing: 0,
                },
                state: Some(FactionState {
                    faction_id: "guild".into(),
                    standing: -2,
                    public_notes: vec!["Watching the player closely".into()],
                    hidden_notes: vec!["Preparing a sealed inquiry".into()],
                    revealed_goals: vec!["Keep order".into()],
                }),
            }],
            active_quests: vec![QuestState {
                quest_id: "mana-anomaly".into(),
                status: QuestStatus::Active,
                completed_objectives: vec!["inspect-rift".into()],
                visible: true,
            }],
            active_clocks: vec![ClockState {
                id: "guildhall-panic".into(),
                title: "Guildhall Panic".into(),
                current: 2,
                max: 6,
                consequence: "The guild seals the hall".into(),
                visible_to_player: true,
            }],
            player_known_facts: vec![Fact {
                id: "known-1".into(),
                text: "Witnesses saw abnormal mana in the guildhall".into(),
                visibility: FactVisibility::PlayerKnown,
                known_by: vec![],
                source: FactSource::Turn,
                reveal_conditions: vec![],
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            }],
            gm_only_facts: vec![
                Fact {
                    id: "gm-relevant".into(),
                    text: "The guildhall panic will expose the hidden ruin".into(),
                    visibility: FactVisibility::GmOnly,
                    known_by: vec![],
                    source: FactSource::Turn,
                    reveal_conditions: vec!["if the panic clock reaches 4".into()],
                    related_secret_ids: vec!["hidden-ruin".into()],
                    reveal_condition_satisfied: None,
                },
                Fact {
                    id: "gm-irrelevant".into(),
                    text: "A duke across the sea hides a sapphire ledger".into(),
                    visibility: FactVisibility::GmOnly,
                    known_by: vec![],
                    source: FactSource::Turn,
                    reveal_conditions: vec!["only during the harbor arc".into()],
                    related_secret_ids: vec!["harbor-ledger".into()],
                    reveal_condition_satisfied: None,
                },
            ],
            recent_summary: Some("The guild suspects the player is tied to the anomaly.".into()),
            recent_messages: vec![
                MessageContext {
                    role: "User".into(),
                    content: "I steady the crowd and ask Seraphyne for help.".into(),
                },
                MessageContext {
                    role: "Assistant".into(),
                    content: "Seraphyne lowers her voice as the hall falls silent.".into(),
                },
            ],
            rules: vec![
                "Do not reveal unrevealed secrets".into(),
                "Respect faction consequences".into(),
            ],
            mode,
        }
    }

    fn snapshot_context(
        style: domain::SceneReasoningStyle,
        priorities: &[&str],
        avoid: &[&str],
        visible_response_shape: &str,
        mode: Option<TurnMode>,
    ) -> crate::AgentContext {
        let mut context = rich_context(mode);
        context.scene_directive = crate::ReasoningStyleDirective {
            style,
            priorities: priorities.iter().map(|value| (*value).to_owned()).collect(),
            avoid: avoid.iter().map(|value| (*value).to_owned()).collect(),
            visible_response_shape: visible_response_shape.into(),
        };
        context
    }

    #[test]
    fn non_streaming_prompt_includes_all_context_sections() {
        let request = BasicPromptBuilder.build_non_streaming_prompt(
            &rich_context(Some(TurnMode::Action)),
            "I negotiate with the guild to stop the panic clock.",
        );
        let user = user_message_content(&request);

        for section in [
            "SCENARIO AND SETTING:",
            "SCENE STYLE DIRECTIVE:",
            "ACTIVE ROLE ACTIVATION:",
            "CURRENT WORLD STATE:",
            "RELEVANT FACTS:",
            "RECENT SUMMARY:",
            "RECENT MESSAGES:",
            "PLAYER INPUT:",
            "OUTPUT CONTRACT:",
        ] {
            assert!(
                user.contains(section),
                "expected section {section} in prompt, got: {user}"
            );
        }
        assert!(user.contains("Seraphyne"));
        assert!(user.contains("Guildhall"));
        assert!(user.contains("track leverage"));
        assert!(user.contains("Solemn and precise"));
    }

    #[test]
    fn streaming_prompt_uses_same_context_without_json_contract() {
        let request = BasicPromptBuilder.build_streaming_prompt(
            &rich_context(Some(TurnMode::Dialogue)),
            "I ask Seraphyne to address the guild.",
        );
        let system = system_message_content(&request);
        let user = user_message_content(&request);

        assert!(system.contains("Stream only player-visible narration"));
        assert!(user.contains("RECENT MESSAGES:"));
        assert!(!user.contains("OUTPUT CONTRACT:"));
        assert!(!user.contains("world_state_delta"));
    }

    #[test]
    fn delta_extraction_prompt_includes_visible_response_and_delta_contract() {
        let request = BasicPromptBuilder.build_delta_extraction_prompt(
            &rich_context(Some(TurnMode::Action)),
            "I calm the room.",
            "Seraphyne raises one hand and the panic ebbs.",
        );
        let user = user_message_content(&request);

        assert!(user.contains("VISIBLE RESPONSE:"));
        assert!(user.contains("Seraphyne raises one hand"));
        assert!(user.contains("Return only world_state_delta JSON."));
        assert!(user.contains("CURRENT WORLD STATE:"));
    }

    #[test]
    fn non_streaming_prompt_filters_irrelevant_gm_only_facts() {
        let request = BasicPromptBuilder.build_non_streaming_prompt(
            &rich_context(Some(TurnMode::Action)),
            "I negotiate with the guild to stop the panic clock.",
        );
        let user = user_message_content(&request);

        assert!(user.contains("The guildhall panic will expose the hidden ruin"));
        assert!(user.contains("if the panic clock reaches 4"));
        assert!(!user.contains("sapphire ledger"));
    }

    #[test]
    fn dialogue_prompt_matches_fixture() {
        let request = BasicPromptBuilder.build_non_streaming_prompt(
            &snapshot_context(
                domain::SceneReasoningStyle::CharacterDialogue,
                &["let the speaker's tone carry the turn", "keep subtext visible"],
                &["dry exposition"],
                "in-world dialogue with immediate interpersonal movement",
                Some(TurnMode::Dialogue),
            ),
            "I ask Seraphyne what she truly fears.",
        );

        assert_eq!(
            user_message_content(&request),
            include_str!("prompt_fixtures/dialogue_user_prompt.txt").trim_end()
        );
    }

    #[test]
    fn political_prompt_matches_fixture() {
        let request = BasicPromptBuilder.build_non_streaming_prompt(
            &snapshot_context(
                domain::SceneReasoningStyle::PoliticalNegotiation,
                &["track leverage", "show visible consequence"],
                &["generic exposition"],
                "immersive dialogue plus visible social consequence",
                Some(TurnMode::Action),
            ),
            "I negotiate with the guild to stop the panic clock.",
        );

        assert_eq!(
            user_message_content(&request),
            include_str!("prompt_fixtures/political_user_prompt.txt").trim_end()
        );
    }

    #[test]
    fn combat_prompt_matches_fixture() {
        let request = BasicPromptBuilder.build_non_streaming_prompt(
            &snapshot_context(
                domain::SceneReasoningStyle::TacticalCombat,
                &["track initiative pressure", "make costs visible"],
                &["static blow-by-blow"],
                "kinetic action with concrete consequences",
                Some(TurnMode::Action),
            ),
            "I lunge across the guildhall and strike before the panic spreads.",
        );

        assert_eq!(
            user_message_content(&request),
            include_str!("prompt_fixtures/combat_user_prompt.txt").trim_end()
        );
    }

    #[test]
    fn mystery_prompt_matches_fixture() {
        let request = BasicPromptBuilder.build_non_streaming_prompt(
            &snapshot_context(
                domain::SceneReasoningStyle::MysteryInvestigation,
                &["surface evidence carefully", "reward observation"],
                &["premature certainty"],
                "observational narration with discoverable leads",
                Some(TurnMode::Action),
            ),
            "I examine the shattered crystal stand for clues.",
        );

        assert_eq!(
            user_message_content(&request),
            include_str!("prompt_fixtures/mystery_user_prompt.txt").trim_end()
        );
    }

    #[test]
    fn rules_prompt_matches_fixture() {
        let request = BasicPromptBuilder.build_non_streaming_prompt(
            &snapshot_context(
                domain::SceneReasoningStyle::RulesAdjudication,
                &["be explicit about mechanics", "stay concise"],
                &["vague rulings"],
                "clear adjudication with direct outcome language",
                Some(TurnMode::Direct),
            ),
            "What rule governs stabilizing the panic clock with a divine ability?",
        );

        assert_eq!(
            user_message_content(&request),
            include_str!("prompt_fixtures/rules_user_prompt.txt").trim_end()
        );
    }
}
