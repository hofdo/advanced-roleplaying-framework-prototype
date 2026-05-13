use crate::{NpcStatus, Scenario};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainValidationError {
    #[error("duplicate {entity} id: {id}")]
    DuplicateId { entity: &'static str, id: String },
    #[error("unknown location id {location_id} for npc {npc_id}")]
    UnknownNpcInitialLocation {
        npc_id: String,
        location_id: String,
    },
    #[error("clock {id} current exceeds max")]
    ClockCurrentExceedsMax { id: String },
    #[error("faction {id} standing must be between -100 and 100")]
    FactionStandingOutOfRange { id: String },
    #[error("dead NPC cannot become active without a revival event")]
    MissingRevivalEvent,
}

pub type DomainValidationResult<T> = Result<T, DomainValidationError>;

pub fn validate_scenario(scenario: &Scenario) -> DomainValidationResult<()> {
    reject_duplicates(
        "location",
        scenario
            .locations
            .iter()
            .map(|location| location.id.as_str()),
    )?;
    reject_duplicates("npc", scenario.npcs.iter().map(|npc| npc.id.as_str()))?;
    reject_duplicates(
        "faction",
        scenario.factions.iter().map(|faction| faction.id.as_str()),
    )?;
    reject_duplicates(
        "quest",
        scenario.quests.iter().map(|quest| quest.id.as_str()),
    )?;
    reject_duplicates(
        "secret",
        scenario.secrets.iter().map(|secret| secret.id.as_str()),
    )?;
    reject_duplicates(
        "clock",
        scenario.clocks.iter().map(|clock| clock.id.as_str()),
    )?;

    for clock in &scenario.clocks {
        if clock.current > clock.max {
            return Err(DomainValidationError::ClockCurrentExceedsMax {
                id: clock.id.clone(),
            });
        }
    }

    for faction in &scenario.factions {
        if !(-100..=100).contains(&faction.initial_standing) {
            return Err(DomainValidationError::FactionStandingOutOfRange {
                id: faction.id.clone(),
            });
        }
    }

    let location_ids = scenario
        .locations
        .iter()
        .map(|location| location.id.as_str())
        .collect::<HashSet<_>>();

    for npc in &scenario.npcs {
        if let Some(location_id) = &npc.initial_location_id
            && !location_ids.contains(location_id.as_str())
        {
            return Err(DomainValidationError::UnknownNpcInitialLocation {
                npc_id: npc.id.clone(),
                location_id: location_id.clone(),
            });
        }
    }

    Ok(())
}

pub fn validate_npc_status_transition(
    from: NpcStatus,
    to: NpcStatus,
    has_revival_event: bool,
) -> DomainValidationResult<()> {
    if from == NpcStatus::Dead && to == NpcStatus::Active && !has_revival_event {
        return Err(DomainValidationError::MissingRevivalEvent);
    }

    Ok(())
}

fn reject_duplicates<'a>(
    entity: &'static str,
    ids: impl Iterator<Item = &'a str>,
) -> DomainValidationResult<()> {
    let mut seen = HashSet::new();

    for id in ids {
        if !seen.insert(id) {
            return Err(DomainValidationError::DuplicateId {
                entity,
                id: id.to_owned(),
            });
        }
    }

    Ok(())
}
