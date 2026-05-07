use domain::{
    ClockChange, EntityKey, FactSource, FactVisibility, FactionChange, LocationChange, NpcChange,
    QuestChange, RelationshipChange, Scenario, WorldState, WorldStateDelta,
    validate_npc_status_transition,
};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ValidatedWorldStateDelta(pub WorldStateDelta);

pub trait DeltaValidator: Send + Sync {
    fn validate(
        &self,
        scenario: &Scenario,
        world_state: &WorldState,
        delta: &WorldStateDelta,
    ) -> Result<ValidatedWorldStateDelta, DeltaValidationError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BasicDeltaValidator;

impl DeltaValidator for BasicDeltaValidator {
    fn validate(
        &self,
        scenario: &Scenario,
        world_state: &WorldState,
        delta: &WorldStateDelta,
    ) -> Result<ValidatedWorldStateDelta, DeltaValidationError> {
        let npc_ids = scenario
            .npcs
            .iter()
            .map(|npc| npc.id.as_str())
            .collect::<HashSet<_>>();
        let faction_ids = scenario
            .factions
            .iter()
            .map(|faction| faction.id.as_str())
            .collect::<HashSet<_>>();
        let quest_ids = scenario
            .quests
            .iter()
            .map(|quest| quest.id.as_str())
            .collect::<HashSet<_>>();
        let clock_ids = world_state
            .clocks
            .iter()
            .map(|clock| clock.id.as_str())
            .collect::<HashSet<_>>();
        let location_ids = scenario
            .locations
            .iter()
            .map(|location| location.id.as_str())
            .collect::<HashSet<_>>();

        for fact in &delta.facts_to_add {
            require_reason(&fact.reason)?;
            if fact.visibility == FactVisibility::PlayerKnown
                && leaks_gm_only_fact(world_state, &fact.text)
            {
                return Err(DeltaValidationError::SecretLeak(fact.text.clone()));
            }
            if fact.visibility == FactVisibility::PlayerKnown
                && !fact.related_secret_ids.is_empty()
                && fact
                    .reveal_condition_satisfied
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .is_none()
            {
                return Err(DeltaValidationError::MissingRevealProof);
            }
        }

        for change in &delta.npc_changes {
            match change {
                NpcChange::AttitudeChanged { npc_id, reason, .. }
                | NpcChange::KnowledgeAdded { npc_id, reason, .. }
                | NpcChange::StatusChanged { npc_id, reason, .. }
                | NpcChange::LocationChanged { npc_id, reason, .. } => {
                    require_known("npc", npc_id, &npc_ids)?;
                    require_reason(reason)?;
                }
            }

            match change {
                NpcChange::KnowledgeAdded {
                    fact,
                    visibility: FactVisibility::PlayerKnown,
                    ..
                } if leaks_gm_only_fact(world_state, fact) => {
                    return Err(DeltaValidationError::SecretLeak(fact.clone()));
                }
                NpcChange::StatusChanged { npc_id, status, .. } => {
                    let current = world_state
                        .npcs
                        .iter()
                        .find(|npc| npc.npc_id == *npc_id)
                        .map(|npc| npc.status)
                        .or_else(|| {
                            scenario
                                .npcs
                                .iter()
                                .find(|npc| npc.id == *npc_id)
                                .map(|npc| npc.initial_status)
                        })
                        .ok_or_else(|| DeltaValidationError::UnknownEntity {
                            entity: "npc",
                            id: npc_id.clone(),
                        })?;
                    validate_npc_status_transition(current, *status, false)
                        .map_err(|error| DeltaValidationError::InvalidStatus(error.to_string()))?;
                }
                NpcChange::LocationChanged { location_id, .. } => {
                    require_known("location", location_id, &location_ids)?;
                }
                _ => {}
            }
        }

        for change in &delta.faction_changes {
            match change {
                FactionChange::StandingChanged {
                    faction_id,
                    standing_delta,
                    reason,
                } => {
                    require_known("faction", faction_id, &faction_ids)?;
                    require_reason(reason)?;
                    let current = world_state
                        .factions
                        .iter()
                        .find(|state| state.faction_id == *faction_id)
                        .map(|state| state.standing)
                        .unwrap_or(0);
                    let next = current + standing_delta;
                    if !(-100..=100).contains(&next) {
                        return Err(DeltaValidationError::StandingOutOfRange {
                            faction_id: faction_id.clone(),
                            value: next,
                        });
                    }
                }
                FactionChange::GoalRevealed {
                    faction_id, reason, ..
                } => {
                    require_known("faction", faction_id, &faction_ids)?;
                    require_reason(reason)?;
                }
            }
        }

        for change in &delta.quest_changes {
            match change {
                QuestChange::Started { quest_id, reason }
                | QuestChange::Completed { quest_id, reason }
                | QuestChange::Failed { quest_id, reason }
                | QuestChange::ObjectiveCompleted {
                    quest_id, reason, ..
                } => {
                    require_known("quest", quest_id, &quest_ids)?;
                    require_reason(reason)?;
                }
            }
        }

        for change in &delta.clock_changes {
            match change {
                ClockChange::Advanced {
                    clock_id,
                    delta,
                    reason,
                } => {
                    require_known("clock", clock_id, &clock_ids)?;
                    require_reason(reason)?;
                    let clock = world_state
                        .clocks
                        .iter()
                        .find(|clock| clock.id == *clock_id)
                        .expect("clock checked");
                    let next = clock.current as i16 + *delta as i16;
                    if next < 0 || next > clock.max as i16 {
                        return Err(DeltaValidationError::ClockOutOfRange {
                            clock_id: clock_id.clone(),
                            value: next,
                        });
                    }
                }
                ClockChange::SetValue {
                    clock_id,
                    value,
                    reason,
                } => {
                    require_known("clock", clock_id, &clock_ids)?;
                    require_reason(reason)?;
                    let clock = world_state
                        .clocks
                        .iter()
                        .find(|clock| clock.id == *clock_id)
                        .expect("clock checked");
                    if *value > clock.max {
                        return Err(DeltaValidationError::ClockOutOfRange {
                            clock_id: clock_id.clone(),
                            value: i16::from(*value),
                        });
                    }
                }
            }
        }

        for change in &delta.relationship_changes {
            let RelationshipChange::Changed {
                source_id,
                target_id,
                reason,
                ..
            } = change;
            require_reason(reason)?;
            if !npc_ids.contains(source_id.as_str()) && !faction_ids.contains(source_id.as_str()) {
                return Err(DeltaValidationError::UnknownEntity {
                    entity: "relationship source",
                    id: source_id.clone(),
                });
            }
            if !npc_ids.contains(target_id.as_str()) && !faction_ids.contains(target_id.as_str()) {
                return Err(DeltaValidationError::UnknownEntity {
                    entity: "relationship target",
                    id: target_id.clone(),
                });
            }
        }

        if let Some(LocationChange {
            location_id,
            reason,
        }) = &delta.location_change
        {
            require_known("location", location_id, &location_ids)?;
            require_reason(reason)?;
        }

        Ok(ValidatedWorldStateDelta(delta.clone()))
    }
}

fn require_known(
    entity: &'static str,
    id: &EntityKey,
    known: &HashSet<&str>,
) -> Result<(), DeltaValidationError> {
    if known.contains(id.as_str()) {
        Ok(())
    } else {
        Err(DeltaValidationError::UnknownEntity {
            entity,
            id: id.clone(),
        })
    }
}

fn require_reason(reason: &str) -> Result<(), DeltaValidationError> {
    if reason.trim().is_empty() {
        Err(DeltaValidationError::MissingReason)
    } else {
        Ok(())
    }
}

fn leaks_gm_only_fact(world_state: &WorldState, text: &str) -> bool {
    world_state.facts.iter().any(|fact| {
        fact.visibility == FactVisibility::GmOnly
            && !fact.text.trim().is_empty()
            && text.to_lowercase().contains(&fact.text.to_lowercase())
            && fact.source != FactSource::PlayerCorrection
    })
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeltaValidationError {
    #[error("unknown {entity} id: {id}")]
    UnknownEntity { entity: &'static str, id: EntityKey },
    #[error("missing required reason")]
    MissingReason,
    #[error("secret leak rejected: {0}")]
    SecretLeak(String),
    #[error("clock {clock_id} value out of range: {value}")]
    ClockOutOfRange { clock_id: EntityKey, value: i16 },
    #[error("faction {faction_id} standing out of range: {value}")]
    StandingOutOfRange { faction_id: EntityKey, value: i32 },
    #[error("invalid NPC status transition: {0}")]
    InvalidStatus(String),
    #[error(
        "PlayerKnown fact references secrets but provides no reveal_condition_satisfied proof"
    )]
    MissingRevealProof,
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::*;
    use uuid::Uuid;

    fn scenario() -> Scenario {
        Scenario {
            id: Uuid::new_v4(),
            title: "Aurethia".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "high fantasy".into(),
            tone: "heroic".into(),
            rules: vec![],
            locations: vec![Location {
                id: "guildhall".into(),
                name: "Guildhall".into(),
                description: "A busy hall.".into(),
                visible: true,
            }],
            factions: vec![Faction {
                id: "guild".into(),
                name: "Guild".into(),
                description: "Adventurers.".into(),
                faction_identity: FactionIdentity {
                    public_goal: "assign quests".into(),
                    hidden_goal: None,
                    values: vec![],
                    fears: vec![],
                    methods: vec![],
                },
                initial_standing: 0,
            }],
            npcs: vec![Npc {
                id: "examiner".into(),
                name: "Examiner".into(),
                description: "Guild examiner.".into(),
                role_identity: RoleIdentity {
                    core_emotion: "alert".into(),
                    motivation: "test applicants".into(),
                    worldview: "contracts matter".into(),
                    fear: None,
                    desire: None,
                    speech_style: "formal".into(),
                    boundaries: vec![],
                    values: vec![],
                },
                stats: None,
                initial_status: NpcStatus::Active,
            }],
            quests: vec![Quest {
                id: "register".into(),
                title: "Register".into(),
                description: "Join the guild.".into(),
                objectives: vec![],
                visible: true,
            }],
            secrets: vec![],
            clocks: vec![],
        }
    }

    fn state() -> WorldState {
        WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            version: 0,
            current_location_id: Some("guildhall".into()),
            current_scene: None,
            active_speaker_id: Some("examiner".into()),
            facts: vec![Fact {
                id: "void-mark".into(),
                text: "The soul-mark was not created by the goddess.".into(),
                visibility: FactVisibility::GmOnly,
                known_by: vec![],
                source: FactSource::Scenario,
                reveal_conditions: vec!["divine relic reacts".into()],
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            }],
            npcs: vec![NpcState {
                npc_id: "examiner".into(),
                status: NpcStatus::Active,
                location_id: Some("guildhall".into()),
                attitude_to_player: None,
                known_facts: vec![],
                notes: vec![],
            }],
            factions: vec![FactionState {
                faction_id: "guild".into(),
                standing: 0,
                public_notes: vec![],
                hidden_notes: vec![],
                revealed_goals: vec![],
            }],
            quests: vec![QuestState {
                quest_id: "register".into(),
                status: QuestStatus::Active,
                completed_objectives: vec![],
                visible: true,
            }],
            clocks: vec![ClockState {
                id: "fame".into(),
                title: "Fame spreads".into(),
                current: 1,
                max: 6,
                consequence: "Factions notice.".into(),
            }],
            relationships: vec![],
            inventory: vec![],
            summary: None,
            recent_events: vec![],
        }
    }

    #[test]
    fn rejects_unknown_npc_id() {
        let delta = WorldStateDelta {
            npc_changes: vec![NpcChange::AttitudeChanged {
                npc_id: "ghost".into(),
                attitude: "curious".into(),
                reason: "The player spoke.".into(),
            }],
            ..WorldStateDelta::default()
        };

        let err = BasicDeltaValidator
            .validate(&scenario(), &state(), &delta)
            .expect_err("unknown npc rejected");

        assert!(matches!(
            err,
            DeltaValidationError::UnknownEntity { entity: "npc", .. }
        ));
    }

    #[test]
    fn rejects_gm_only_fact_becoming_player_known() {
        let delta = WorldStateDelta {
            facts_to_add: vec![FactToAdd {
                text: "The soul-mark was not created by the goddess.".into(),
                visibility: FactVisibility::PlayerKnown,
                known_by: vec![],
                reveal_conditions: vec![],
                reason: "The model revealed it early.".into(),
                related_secret_ids: vec![],
                reveal_condition_satisfied: None,
            }],
            ..WorldStateDelta::default()
        };

        let err = BasicDeltaValidator
            .validate(&scenario(), &state(), &delta)
            .expect_err("secret leak rejected");

        assert!(matches!(err, DeltaValidationError::SecretLeak(_)));
    }

    #[test]
    fn player_known_fact_with_secret_ref_requires_reveal_proof() {
        let delta = WorldStateDelta {
            facts_to_add: vec![FactToAdd {
                text: "The hero learned something new.".into(),
                visibility: FactVisibility::PlayerKnown,
                known_by: vec![],
                reveal_conditions: vec![],
                reason: "Observed during the scene.".into(),
                related_secret_ids: vec!["secret-vault".into()],
                reveal_condition_satisfied: None,
            }],
            ..WorldStateDelta::default()
        };

        let err = BasicDeltaValidator
            .validate(&scenario(), &state(), &delta)
            .expect_err("missing reveal proof rejected");

        assert!(matches!(err, DeltaValidationError::MissingRevealProof));
    }

    #[test]
    fn player_known_fact_with_secret_ref_and_proof_passes() {
        let delta = WorldStateDelta {
            facts_to_add: vec![FactToAdd {
                text: "The hero learned something new.".into(),
                visibility: FactVisibility::PlayerKnown,
                known_by: vec![],
                reveal_conditions: vec![],
                reason: "Observed during the scene.".into(),
                related_secret_ids: vec!["secret-vault".into()],
                reveal_condition_satisfied: Some("revealed via secret-vault".into()),
            }],
            ..WorldStateDelta::default()
        };

        BasicDeltaValidator
            .validate(&scenario(), &state(), &delta)
            .expect("valid delta with reveal proof should pass");
    }

    #[test]
    fn gm_only_fact_with_secret_ref_no_proof_passes() {
        let delta = WorldStateDelta {
            facts_to_add: vec![FactToAdd {
                text: "The villain controls the guild.".into(),
                visibility: FactVisibility::GmOnly,
                known_by: vec![],
                reveal_conditions: vec![],
                reason: "GM background knowledge.".into(),
                related_secret_ids: vec!["secret-vault".into()],
                reveal_condition_satisfied: None,
            }],
            ..WorldStateDelta::default()
        };

        BasicDeltaValidator
            .validate(&scenario(), &state(), &delta)
            .expect("GM-only fact with secret ref and no proof should pass");
    }
}
