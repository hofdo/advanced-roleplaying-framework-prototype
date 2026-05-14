use crate::AgentContext;
use std::collections::HashSet;

pub(super) fn render_context(context: &AgentContext, player_input: &str) -> String {
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
        render_facts_section(context, player_input, true),
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

pub(super) fn render_narration_context(context: &AgentContext, player_input: &str) -> String {
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
        render_facts_section(context, player_input, false),
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

fn render_facts_section(
    context: &AgentContext,
    player_input: &str,
    include_gm_only: bool,
) -> String {
    let player_known = format!(
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
    );
    if include_gm_only {
        render_section(
            "RELEVANT FACTS",
            &[
                player_known,
                format!(
                    "GM-only facts (do not reveal unless justified): {}",
                    render_gm_only_facts(context, player_input)
                ),
            ],
        )
    } else {
        render_section("RELEVANT FACTS", &[player_known])
    }
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
    let active_role_name = context
        .active_role
        .active_role_name
        .as_deref()
        .unwrap_or("");
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
