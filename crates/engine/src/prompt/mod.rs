use crate::AgentContext;
use domain::TurnMode;
use providers::{LlmMessage, LlmMessageRole, LlmRequest};

pub mod parser;
mod render;
pub mod repair;

pub use parser::{JsonResponseParser, ParseError, PlayerTurnModelOutput, ResponseParser};
use render::{render_context, render_narration_context};
pub use repair::repair_prompt;

pub const PROMPT_TEMPLATE_VERSION: &str = "roleplaying-engine-v2";

pub trait PromptBuilder: Send + Sync {
    fn build_non_streaming_prompt(&self, context: &AgentContext, player_input: &str) -> LlmRequest;
    fn build_streaming_prompt(&self, context: &AgentContext, player_input: &str) -> LlmRequest;
    /// Build a narration-only prompt for non-streaming visible response generation.
    ///
    /// Uses `render_narration_context` so GM-only facts are excluded. Pairs with
    /// `build_delta_extraction_prompt` for the oracle-context follow-up call.
    fn build_visible_response_prompt(
        &self,
        context: &AgentContext,
        player_input: &str,
    ) -> LlmRequest;
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
                        "{}\n\nOUTPUT CONTRACT:\nReturn strict JSON with keys player_response and world_state_delta. The delta may only use typed change arrays. For risky action turns, include action_resolution_changes with intent, stakes, outcome, and consequence.",
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
        narration_request(
            context,
            player_input,
            "Stream only player-visible narration. Do not emit JSON.",
            0.8,
        )
    }

    fn build_visible_response_prompt(
        &self,
        context: &AgentContext,
        player_input: &str,
    ) -> LlmRequest {
        narration_request(
            context,
            player_input,
            "Return only the player-visible narration. Do not emit JSON or delta fields.",
            0.7,
        )
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
                        "{}\n\nVISIBLE RESPONSE:\n{}\n\nOUTPUT CONTRACT:\nReturn only world_state_delta JSON. For risky action turns, include action_resolution_changes with intent, stakes, outcome, and consequence.",
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

fn narration_request(
    context: &AgentContext,
    player_input: &str,
    system_suffix: &str,
    temperature: f32,
) -> LlmRequest {
    LlmRequest {
        messages: vec![
            LlmMessage {
                role: LlmMessageRole::System,
                content: format!("{}\n{}", system_rules(context.mode), system_suffix),
            },
            LlmMessage {
                role: LlmMessageRole::User,
                content: render_narration_context(context, player_input),
            },
        ],
        temperature: Some(temperature),
        max_tokens: None,
        json_mode: false,
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
        Some(TurnMode::Action) => Some(
            "The player is performing an in-world action. Narrate the outcome. For risky actions, track stakes and make the consequence visible.",
        ),
        Some(TurnMode::Direct) => Some(
            "The player is asking an out-of-character question. Answer as GM directly and clearly. Do not stay in character.",
        ),
        Some(TurnMode::Remember) => Some(
            "The player is providing a memory or fact correction. Acknowledge it, update your understanding, and confirm what changed.",
        ),
        // Dialogue and None: no preamble — character dialogue is the default
        Some(TurnMode::Dialogue) | None => None,
    };

    match preamble {
        Some(text) => format!("{text}\n{base}"),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            offscreen_npc_activity: vec![],
            relevant_factions: vec![],
            relevant_relationships: vec![],
            active_quests: vec![],
            active_clocks: vec![],
            recent_action_resolutions: vec![],
            visible_clues: vec![],
            player_state: domain::PlayerCharacterState::default(),
            player_known_facts: vec![],
            gm_only_facts: vec![],
            player_memories: vec![],
            gm_only_memories: vec![],
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

    fn context_with_secret(secret_text: &str) -> crate::AgentContext {
        use domain::{Fact, FactSource, FactVisibility};
        let mut ctx = minimal_context(None);
        ctx.gm_only_facts.push(Fact {
            id: "secret-1".into(),
            text: secret_text.into(),
            visibility: FactVisibility::GmOnly,
            known_by: vec![],
            source: FactSource::Scenario,
            reveal_conditions: vec![domain::RevealCondition {
                id: "inspect-treaty".into(),
                description: "The chancellor inspects the treaty.".into(),
            }],
            related_secret_ids: vec![],
            reveal_condition_satisfied: None,
        });
        ctx
    }

    fn context_with_memories() -> crate::AgentContext {
        use domain::{MemoryEntry, MemoryVisibility};

        let mut ctx = minimal_context(None);
        ctx.player_memories.push(MemoryEntry {
            id: "memory-public".into(),
            text: "Marta noticed the player treat the staff well.".into(),
            visibility: MemoryVisibility::PlayerKnown,
            importance: 6,
            related_entity_ids: vec!["steward-marta".into()],
            source_message_id: None,
        });
        ctx.gm_only_memories.push(MemoryEntry {
            id: "memory-gm".into(),
            text: "The guild quietly flagged the player as a risk.".into(),
            visibility: MemoryVisibility::GmOnly,
            importance: 9,
            related_entity_ids: vec!["guild".into()],
            source_message_id: None,
        });
        ctx
    }

    #[test]
    fn non_streaming_visible_prompt_excludes_gm_only_facts() {
        let context = context_with_secret("The chancellor poisoned the treaty.");
        let request =
            BasicPromptBuilder.build_visible_response_prompt(&context, "I greet the chancellor.");
        let combined = request
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(!combined.contains("The chancellor poisoned the treaty."));
        assert!(combined.contains("Player-known facts"));
        assert!(!request.json_mode);
    }

    #[test]
    fn delta_extraction_prompt_includes_gm_only_facts_for_chancellor_secret() {
        let context = context_with_secret("The chancellor poisoned the treaty.");
        let request = BasicPromptBuilder.build_delta_extraction_prompt(
            &context,
            "I inspect the treaty.",
            "The seal smells faintly bitter.",
        );
        let combined = request
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(combined.contains("The chancellor poisoned the treaty."));
        assert!(request.json_mode);
    }

    #[test]
    fn visible_response_prompt_includes_player_memory_only() {
        let request = BasicPromptBuilder.build_visible_response_prompt(
            &context_with_memories(),
            "I ask Marta what she thinks.",
        );
        let combined = request
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(combined.contains("Marta noticed the player treat the staff well."));
        assert!(!combined.contains("The guild quietly flagged the player as a risk."));
    }

    #[test]
    fn delta_extraction_prompt_includes_gm_only_memory() {
        let request = BasicPromptBuilder.build_delta_extraction_prompt(
            &context_with_memories(),
            "I ask Marta what she thinks.",
            "Marta studies you more carefully before answering.",
        );
        let combined = request
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(combined.contains("Marta noticed the player treat the staff well."));
        assert!(combined.contains("The guild quietly flagged the player as a risk."));
    }

    fn rich_context(mode: Option<TurnMode>) -> crate::AgentContext {
        use crate::{
            FactionContext, MessageContext, NpcContext, ReasoningStyleDirective,
            RoleActivationContext,
        };
        use domain::{
            ClockState, Fact, FactSource, FactVisibility, Faction, FactionIdentity, FactionState,
            Location, Npc, NpcStatus, QuestState, QuestStatus, RoleIdentity, SceneReasoningStyle,
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
                visible_response_shape: "immersive dialogue plus visible social consequence".into(),
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
                    initial_location_id: None,
                    initial_visible_to_player: true,
                },
                status: NpcStatus::Active,
                attitude_to_player: Some("cautious warmth".into()),
            }],
            offscreen_npc_activity: vec![crate::OffscreenNpcActivity {
                npc_name: "Captain Roderic".into(),
                intent: "secure the upper galleries".into(),
                result: "quietly moved guards into position".into(),
                visible_to_player: false,
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
                    pressure: 3,
                    public_pressure_notes: vec!["The guild wants visible control.".into()],
                    hidden_pressure_notes: vec!["Leaders fear a deeper anomaly.".into()],
                }),
            }],
            relevant_relationships: vec![domain::RelationshipState {
                source_id: "seraphyne".into(),
                target_id: "player".into(),
                attitude: 2,
                notes: vec!["Measured trust".into()],
                trust: 3,
                suspicion: 1,
                loyalty: 0,
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
            recent_action_resolutions: vec![domain::ActionResolution {
                id: "action-2-1".into(),
                intent: "Stabilize the mana surge before the crowd panics.".into(),
                stakes: vec!["the guildhall may erupt".into()],
                outcome: domain::ActionOutcome::SuccessWithCost,
                consequence: "The surge is contained, but the guild now watches closely.".into(),
                visible_to_player: true,
                linked_clock_ids: vec!["guildhall-panic".into()],
            }],
            visible_clues: vec![domain::ClueState {
                id: "shattered-stand".into(),
                text: "The shattered crystal stand was sabotaged from inside the guildhall.".into(),
                linked_secret_ids: vec!["hidden-ruin".into()],
                satisfied_reveal_conditions: vec![domain::ConditionRef {
                    id: "inspect-stand".into(),
                    mode: domain::MatchMode::Exact,
                }],
                visible_to_player: true,
            }],
            player_state: domain::PlayerCharacterState {
                traits: vec![domain::PlayerTrait {
                    id: "divine-marked".into(),
                    label: "Divine Marked".into(),
                    description: "Carries an unstable soul-mark.".into(),
                    visible_to_player: true,
                }],
                goals: vec![domain::PlayerGoal {
                    id: "calm-the-hall".into(),
                    label: "Calm the Hall".into(),
                    description: "Prevent the guildhall from collapsing into panic.".into(),
                    progress: 40,
                    visible_to_player: true,
                }],
                conditions: vec![domain::PlayerCondition {
                    id: "drained".into(),
                    label: "Drained".into(),
                    description: "The recent surge left the body shaking.".into(),
                    visible_to_player: true,
                }],
                resources: vec![domain::PlayerResource {
                    id: "resolve".into(),
                    label: "Resolve".into(),
                    current: 3,
                    min: 0,
                    max: 5,
                    visible_to_player: true,
                }],
                gm_notes: vec!["The player still hides how unstable the mark feels.".into()],
            },
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
                    reveal_conditions: vec![domain::RevealCondition {
                        id: "panic-clock-four".into(),
                        description: "if the panic clock reaches 4".into(),
                    }],
                    related_secret_ids: vec!["hidden-ruin".into()],
                    reveal_condition_satisfied: None,
                },
                Fact {
                    id: "gm-irrelevant".into(),
                    text: "A duke across the sea hides a sapphire ledger".into(),
                    visibility: FactVisibility::GmOnly,
                    known_by: vec![],
                    source: FactSource::Turn,
                    reveal_conditions: vec![domain::RevealCondition {
                        id: "harbor-arc".into(),
                        description: "only during the harbor arc".into(),
                    }],
                    related_secret_ids: vec!["harbor-ledger".into()],
                    reveal_condition_satisfied: None,
                },
            ],
            player_memories: vec![domain::MemoryEntry {
                id: "memory-known-1".into(),
                text: "The guild watched the player stabilize the mana surge.".into(),
                visibility: domain::MemoryVisibility::PlayerKnown,
                importance: 6,
                related_entity_ids: vec!["guild".into()],
                source_message_id: None,
            }],
            gm_only_memories: vec![domain::MemoryEntry {
                id: "memory-gm-1".into(),
                text: "Seraphyne fears the anomaly is linked to the hidden ruin.".into(),
                visibility: domain::MemoryVisibility::GmOnly,
                importance: 9,
                related_entity_ids: vec!["hidden-ruin".into()],
                source_message_id: None,
            }],
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
    fn streaming_prompt_excludes_gm_only_facts() {
        let request = BasicPromptBuilder.build_streaming_prompt(
            &rich_context(Some(TurnMode::Action)),
            "I approach Seraphyne and demand the truth.",
        );
        let user = user_message_content(&request);

        assert!(
            !user.contains("GM-only facts"),
            "streaming prompt must not contain GM-only facts section"
        );
        assert!(
            !user.contains("The guildhall panic will expose the hidden ruin"),
            "streaming prompt must not contain GM-only fact text"
        );
        assert!(
            user.contains("RELEVANT FACTS:"),
            "player-known facts section must still be present"
        );
        assert!(
            user.contains("Witnesses saw abnormal mana"),
            "player-known facts must still appear"
        );
    }

    #[test]
    fn non_streaming_prompt_includes_gm_only_facts() {
        let request = BasicPromptBuilder.build_non_streaming_prompt(
            &rich_context(Some(TurnMode::Action)),
            "I approach Seraphyne and demand the truth.",
        );
        let user = user_message_content(&request);

        assert!(
            user.contains("GM-only facts (do not reveal unless justified):"),
            "non-streaming (oracle) prompt must retain GM-only facts section"
        );
    }

    #[test]
    fn delta_extraction_prompt_includes_gm_only_facts() {
        let request = BasicPromptBuilder.build_delta_extraction_prompt(
            &rich_context(Some(TurnMode::Action)),
            "I approach Seraphyne and demand the truth.",
            "Seraphyne meets your gaze but says nothing.",
        );
        let user = user_message_content(&request);

        assert!(
            user.contains("GM-only facts (do not reveal unless justified):"),
            "delta extraction (oracle) prompt must retain GM-only facts section"
        );
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
                &[
                    "let the speaker's tone carry the turn",
                    "keep subtext visible",
                ],
                &["dry exposition"],
                "in-world dialogue with immediate interpersonal movement",
                Some(TurnMode::Dialogue),
            ),
            "I ask Seraphyne what she truly fears.",
        );

        assert_eq!(
            user_message_content(&request),
            include_str!("../prompt_fixtures/dialogue_user_prompt.txt").trim_end()
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
            include_str!("../prompt_fixtures/political_user_prompt.txt").trim_end()
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
            include_str!("../prompt_fixtures/combat_user_prompt.txt").trim_end()
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
            include_str!("../prompt_fixtures/mystery_user_prompt.txt").trim_end()
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
            include_str!("../prompt_fixtures/rules_user_prompt.txt").trim_end()
        );
    }
}
