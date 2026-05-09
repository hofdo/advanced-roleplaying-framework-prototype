use domain::{
    ClockChange, EntityKey, FactSource, FactVisibility, FactionChange, InventoryChange,
    LocationChange, NpcChange, NpcStatus, QuestChange, RelationshipChange, Scenario, WorldState,
    WorldStateDelta,
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

        if let Some(scene_change) = &delta.scene_change {
            require_reason(&scene_change.reason)?;
        }

        if let Some(active_speaker_change) = &delta.active_speaker_change {
            require_reason(&active_speaker_change.reason)?;
            if let Some(speaker_id) = &active_speaker_change.speaker_id {
                require_known("npc", speaker_id, &npc_ids)?;
            }
        }

        if let Some(summary_update) = &delta.summary_update {
            require_reason(&summary_update.reason)?;
        }

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
            let npc_id = match change {
                NpcChange::AttitudeChanged { npc_id, .. }
                | NpcChange::KnowledgeAdded { npc_id, .. }
                | NpcChange::StatusChanged { npc_id, .. }
                | NpcChange::LocationChanged { npc_id, .. }
                | NpcChange::NoteAdded { npc_id, .. } => npc_id,
            };

            match change {
                NpcChange::AttitudeChanged { reason, .. }
                | NpcChange::KnowledgeAdded { reason, .. }
                | NpcChange::StatusChanged { reason, .. }
                | NpcChange::LocationChanged { reason, .. }
                | NpcChange::NoteAdded { reason, .. } => {
                    require_known("npc", npc_id, &npc_ids)?;
                    require_reason(reason)?;
                }
            }

            // Check that the proposed change is allowed given the NPC's current status.
            let current_npc = world_state.npcs.iter().find(|n| n.npc_id == *npc_id);
            if let Some(npc) = current_npc {
                match change {
                    NpcChange::KnowledgeAdded { .. } | NpcChange::AttitudeChanged { .. } => {
                        if matches!(npc.status, NpcStatus::Unconscious | NpcStatus::Dead) {
                            return Err(DeltaValidationError::InvalidNpcStatusAction {
                                npc_id: npc_id.clone(),
                                status: npc.status,
                                action: "knowledge or attitude change".into(),
                            });
                        }
                    }
                    NpcChange::LocationChanged { .. } => {
                        if npc.status == NpcStatus::Dead {
                            return Err(DeltaValidationError::InvalidNpcStatusAction {
                                npc_id: npc_id.clone(),
                                status: npc.status,
                                action: "location change".into(),
                            });
                        }
                    }
                    NpcChange::NoteAdded { .. } => {}
                    NpcChange::StatusChanged { .. } => {} // Always allowed — this is how you change status
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
                FactionChange::PublicNoteAdded {
                    faction_id, reason, ..
                }
                | FactionChange::HiddenNoteAdded {
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
                ClockChange::VisibilityChanged {
                    clock_id, reason, ..
                } => {
                    require_known("clock", clock_id, &clock_ids)?;
                    require_reason(reason)?;
                }
            }
        }

        for change in &delta.relationship_changes {
            let (source_id, target_id, reason) = match change {
                RelationshipChange::Changed {
                    source_id,
                    target_id,
                    reason,
                    ..
                }
                | RelationshipChange::NoteAdded {
                    source_id,
                    target_id,
                    reason,
                    ..
                } => (source_id, target_id, reason),
            };
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

        let inventory_ids = world_state
            .inventory
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        for change in &delta.inventory_changes {
            match change {
                InventoryChange::Added { item, reason } => {
                    require_reason(reason)?;
                    if item.id.trim().is_empty() {
                        return Err(DeltaValidationError::UnknownEntity {
                            entity: "inventory item",
                            id: item.id.clone(),
                        });
                    }
                }
                InventoryChange::Removed { item_id, reason } => {
                    require_reason(reason)?;
                    require_known("inventory item", item_id, &inventory_ids)?;
                }
                InventoryChange::Updated { item, reason } => {
                    require_reason(reason)?;
                    require_known("inventory item", &item.id, &inventory_ids)?;
                }
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
    #[error("NPC {npc_id} (status: {status:?}) cannot perform {action}")]
    InvalidNpcStatusAction {
        npc_id: EntityKey,
        status: NpcStatus,
        action: String,
    },
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
                visible_to_player: true,
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
                visible_to_player: true,
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

    // --- NPC status action restriction tests ---

    fn state_with_npc_status(status: NpcStatus) -> WorldState {
        let mut s = state();
        s.npcs[0].status = status;
        s
    }

    #[test]
    fn dead_npc_cannot_gain_knowledge() {
        let world = state_with_npc_status(NpcStatus::Dead);
        let delta = WorldStateDelta {
            npc_changes: vec![NpcChange::KnowledgeAdded {
                npc_id: "examiner".into(),
                fact: "The guild has a vault.".into(),
                visibility: FactVisibility::GmOnly,
                reason: "The examiner somehow learned this.".into(),
            }],
            ..WorldStateDelta::default()
        };

        let err = BasicDeltaValidator
            .validate(&scenario(), &world, &delta)
            .expect_err("dead NPC gaining knowledge must be rejected");

        assert!(
            matches!(
                err,
                DeltaValidationError::InvalidNpcStatusAction { .. }
            ),
            "expected InvalidNpcStatusAction, got: {err:?}"
        );
    }

    #[test]
    fn dead_npc_cannot_change_attitude() {
        let world = state_with_npc_status(NpcStatus::Dead);
        let delta = WorldStateDelta {
            npc_changes: vec![NpcChange::AttitudeChanged {
                npc_id: "examiner".into(),
                attitude: "hostile".into(),
                reason: "The examiner turned hostile posthumously.".into(),
            }],
            ..WorldStateDelta::default()
        };

        let err = BasicDeltaValidator
            .validate(&scenario(), &world, &delta)
            .expect_err("dead NPC attitude change must be rejected");

        assert!(
            matches!(
                err,
                DeltaValidationError::InvalidNpcStatusAction { .. }
            ),
            "expected InvalidNpcStatusAction, got: {err:?}"
        );
    }

    #[test]
    fn dead_npc_cannot_move() {
        let world = state_with_npc_status(NpcStatus::Dead);
        let delta = WorldStateDelta {
            npc_changes: vec![NpcChange::LocationChanged {
                npc_id: "examiner".into(),
                location_id: "guildhall".into(),
                reason: "The corpse walked over somehow.".into(),
            }],
            ..WorldStateDelta::default()
        };

        let err = BasicDeltaValidator
            .validate(&scenario(), &world, &delta)
            .expect_err("dead NPC location change must be rejected");

        assert!(
            matches!(
                err,
                DeltaValidationError::InvalidNpcStatusAction { .. }
            ),
            "expected InvalidNpcStatusAction, got: {err:?}"
        );
    }

    #[test]
    fn dead_npc_status_change_allowed() {
        let world = state_with_npc_status(NpcStatus::Dead);
        // Changing from Dead to Injured simulates a resurrection/revival.
        // validate_npc_status_transition only blocks Dead->Active without revival,
        // so Dead->Injured is permitted at this layer.
        let delta = WorldStateDelta {
            npc_changes: vec![NpcChange::StatusChanged {
                npc_id: "examiner".into(),
                status: NpcStatus::Injured,
                reason: "A cleric cast revivify on the examiner.".into(),
            }],
            ..WorldStateDelta::default()
        };

        BasicDeltaValidator
            .validate(&scenario(), &world, &delta)
            .expect("StatusChanged on a dead NPC must be allowed (revival path)");
    }

    #[test]
    fn unconscious_npc_cannot_gain_knowledge() {
        let world = state_with_npc_status(NpcStatus::Unconscious);
        let delta = WorldStateDelta {
            npc_changes: vec![NpcChange::KnowledgeAdded {
                npc_id: "examiner".into(),
                fact: "The guild has a vault.".into(),
                visibility: FactVisibility::GmOnly,
                reason: "The examiner somehow absorbed this while unconscious.".into(),
            }],
            ..WorldStateDelta::default()
        };

        let err = BasicDeltaValidator
            .validate(&scenario(), &world, &delta)
            .expect_err("unconscious NPC gaining knowledge must be rejected");

        assert!(
            matches!(
                err,
                DeltaValidationError::InvalidNpcStatusAction { .. }
            ),
            "expected InvalidNpcStatusAction, got: {err:?}"
        );
    }

    #[test]
    fn active_npc_can_gain_knowledge() {
        let world = state_with_npc_status(NpcStatus::Active);
        let delta = WorldStateDelta {
            npc_changes: vec![NpcChange::KnowledgeAdded {
                npc_id: "examiner".into(),
                fact: "The guild has a vault.".into(),
                visibility: FactVisibility::GmOnly,
                reason: "The examiner overheard a conversation.".into(),
            }],
            ..WorldStateDelta::default()
        };

        BasicDeltaValidator
            .validate(&scenario(), &world, &delta)
            .expect("active NPC gaining knowledge must be allowed");
    }

    #[test]
    fn validator_accepts_new_state_mutation_variants_when_entities_exist() {
        let mut world = state();
        world.current_scene = Some("dialogue".into());
        world.active_speaker_id = Some("examiner".into());
        world.factions.push(FactionState {
            faction_id: "guild".into(),
            standing: 0,
            public_notes: vec![],
            hidden_notes: vec![],
            revealed_goals: vec![],
        });
        world.relationships.push(RelationshipState {
            source_id: "examiner".into(),
            target_id: "guild".into(),
            attitude: 0,
            notes: vec![],
        });
        world.clocks.push(ClockState {
            id: "fame".into(),
            title: "Fame".into(),
            current: 1,
            max: 6,
            consequence: "Witnesses notice.".into(),
            visible_to_player: true,
        });

        let delta = WorldStateDelta {
            scene_change: Some(domain::SceneChange {
                scene: Some("combat".into()),
                reason: "The confrontation escalated.".into(),
            }),
            active_speaker_change: Some(domain::ActiveSpeakerChange {
                speaker_id: Some("examiner".into()),
                reason: "The examiner took command.".into(),
            }),
            summary_update: Some(domain::SummaryUpdate {
                summary: Some("The guildhall confrontation turned violent.".into()),
                reason: "The turn summary should persist.".into(),
            }),
            inventory_changes: vec![domain::InventoryChange::Added {
                item: domain::InventoryItem {
                    id: "ritual-knife".into(),
                    name: "Ritual Knife".into(),
                    description: "Warm and humming.".into(),
                    visible: true,
                },
                reason: "The player picked it up.".into(),
            }],
            npc_changes: vec![NpcChange::NoteAdded {
                npc_id: "examiner".into(),
                note: "The player remains unstable.".into(),
                reason: "Persistent NPC memory.".into(),
            }],
            faction_changes: vec![
                FactionChange::PublicNoteAdded {
                    faction_id: "guild".into(),
                    note: "The guildhall is on alert.".into(),
                    reason: "Persistent public faction memory.".into(),
                },
                FactionChange::HiddenNoteAdded {
                    faction_id: "guild".into(),
                    note: "An internal inquiry began.".into(),
                    reason: "Persistent hidden faction memory.".into(),
                },
            ],
            relationship_changes: vec![RelationshipChange::NoteAdded {
                source_id: "examiner".into(),
                target_id: "guild".into(),
                note: "The examiner now reports directly to guild masters.".into(),
                reason: "Persistent relationship memory.".into(),
            }],
            clock_changes: vec![ClockChange::VisibilityChanged {
                clock_id: "fame".into(),
                visible_to_player: false,
                reason: "The clock should be hidden from players.".into(),
            }],
            ..WorldStateDelta::default()
        };

        let result = BasicDeltaValidator.validate(&scenario(), &world, &delta);

        assert!(result.is_ok());
    }
}
