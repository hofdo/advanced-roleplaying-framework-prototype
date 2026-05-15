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
        render_player_state_section(context, true),
        render_social_state_section(context, true),
        render_npc_agency_section(context, true),
        render_action_resolution_section(context),
        render_clue_section(context),
        render_memory_section(context, true),
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
        render_player_state_section(context, false),
        render_social_state_section(context, false),
        render_npc_agency_section(context, false),
        render_action_resolution_section(context),
        render_clue_section(context),
        render_memory_section(context, false),
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

fn render_memory_section(context: &AgentContext, include_gm_only: bool) -> String {
    let player_visible = format!(
        "Player-visible memory: {}",
        if context.player_memories.is_empty() {
            "none".into()
        } else {
            context
                .player_memories
                .iter()
                .map(format_memory)
                .collect::<Vec<_>>()
                .join(" | ")
        }
    );

    if include_gm_only {
        render_section(
            "CAMPAIGN MEMORY",
            &[
                player_visible,
                format!(
                    "GM-only memory: {}",
                    if context.gm_only_memories.is_empty() {
                        "none".into()
                    } else {
                        context
                            .gm_only_memories
                            .iter()
                            .map(format_memory)
                            .collect::<Vec<_>>()
                            .join(" | ")
                    }
                ),
            ],
        )
    } else {
        render_section("CAMPAIGN MEMORY", &[player_visible])
    }
}

fn render_player_state_section(context: &AgentContext, include_gm_only: bool) -> String {
    let traits = if context.player_state.traits.is_empty() {
        "none".into()
    } else {
        context
            .player_state
            .traits
            .iter()
            .map(|item| format!("{}: {}", item.label, item.description))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let goals = if context.player_state.goals.is_empty() {
        "none".into()
    } else {
        context
            .player_state
            .goals
            .iter()
            .map(|item| format!("{} (progress: {})", item.label, item.progress))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let conditions = if context.player_state.conditions.is_empty() {
        "none".into()
    } else {
        context
            .player_state
            .conditions
            .iter()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let resources = if context.player_state.resources.is_empty() {
        "none".into()
    } else {
        context
            .player_state
            .resources
            .iter()
            .map(|item| format!("{} {}/{}", item.label, item.current, item.max))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let mut lines = vec![
        format!("Traits: {traits}"),
        format!("Goals: {goals}"),
        format!("Conditions: {conditions}"),
        format!("Resources: {resources}"),
    ];
    if include_gm_only {
        lines.push(format!(
            "GM notes: {}",
            if context.player_state.gm_notes.is_empty() {
                "none".into()
            } else {
                context.player_state.gm_notes.join(" | ")
            }
        ));
    }
    render_section("PLAYER CHARACTER", &lines)
}

fn render_social_state_section(context: &AgentContext, include_gm_only: bool) -> String {
    let relationships = if context.relevant_relationships.is_empty() {
        "none".into()
    } else {
        context
            .relevant_relationships
            .iter()
            .map(|relationship| {
                format!(
                    "{} -> {} (attitude {}; trust {}; suspicion {}; loyalty {})",
                    relationship.source_id,
                    relationship.target_id,
                    relationship.attitude,
                    relationship.trust,
                    relationship.suspicion,
                    relationship.loyalty
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let factions = if context.relevant_factions.is_empty() {
        "none".into()
    } else {
        context
            .relevant_factions
            .iter()
            .map(|faction| {
                let Some(state) = &faction.state else {
                    return faction.faction.name.clone();
                };
                let mut summary = format!(
                    "{} (standing {}; pressure {})",
                    faction.faction.name, state.standing, state.pressure
                );
                if !state.public_pressure_notes.is_empty() {
                    summary.push_str(&format!(
                        "; public pressure: {}",
                        state.public_pressure_notes.join(" / ")
                    ));
                }
                if include_gm_only && !state.hidden_pressure_notes.is_empty() {
                    summary.push_str(&format!(
                        "; hidden pressure: {}",
                        state.hidden_pressure_notes.join(" / ")
                    ));
                }
                summary
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    render_section(
        "SOCIAL STATE",
        &[
            format!("Relationships: {relationships}"),
            format!("Faction pressure: {factions}"),
        ],
    )
}

fn render_npc_agency_section(context: &AgentContext, include_hidden: bool) -> String {
    let present = if context.relevant_npcs.is_empty() {
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
    };
    let offscreen = context
        .offscreen_npc_activity
        .iter()
        .filter(|activity| include_hidden || activity.visible_to_player)
        .map(|activity| {
            format!(
                "{}: {} -> {}",
                activity.npc_name, activity.intent, activity.result
            )
        })
        .collect::<Vec<_>>();
    render_section(
        "NPC AGENCY",
        &[
            format!("Present NPCs: {present}"),
            format!(
                "Offscreen activity: {}",
                if offscreen.is_empty() {
                    "none".into()
                } else {
                    offscreen.join(" | ")
                }
            ),
        ],
    )
}

fn render_action_resolution_section(context: &AgentContext) -> String {
    render_section(
        "ACTION RESOLUTIONS",
        &[if context.recent_action_resolutions.is_empty() {
            "none".into()
        } else {
            context
                .recent_action_resolutions
                .iter()
                .map(|resolution| {
                    format!(
                        "{} -> {:?} ({})",
                        resolution.intent, resolution.outcome, resolution.consequence
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ")
        }],
    )
}

fn render_clue_section(context: &AgentContext) -> String {
    render_section(
        "DISCOVERED CLUES",
        &[if context.visible_clues.is_empty() {
            "none".into()
        } else {
            context
                .visible_clues
                .iter()
                .map(|clue| clue.text.clone())
                .collect::<Vec<_>>()
                .join(" | ")
        }],
    )
}

fn format_memory(memory: &domain::MemoryEntry) -> String {
    let related = if memory.related_entity_ids.is_empty() {
        String::new()
    } else {
        format!(" [related: {}]", memory.related_entity_ids.join(", "))
    };
    format!(
        "{} (importance: {}){}",
        memory.text, memory.importance, related
    )
}

fn format_reveal_conditions(reveal_conditions: &[domain::RevealCondition]) -> String {
    if reveal_conditions.is_empty() {
        "none".into()
    } else {
        reveal_conditions
            .iter()
            .map(|condition| condition.description.clone())
            .collect::<Vec<_>>()
            .join("; ")
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
                let reveal_conditions = format_reveal_conditions(&fact.reveal_conditions);
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
            let fact_text = format!(
                "{} {}",
                fact.text,
                fact.reveal_conditions
                    .iter()
                    .map(|condition| format!("{} {}", condition.id, condition.description))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
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
