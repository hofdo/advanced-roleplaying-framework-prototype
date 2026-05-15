use anyhow::Result;
use clap::Subcommand;
use domain::{ViewerContext, WorldState};
use engine::FrontendStateProjector;
use persistence::SessionRecord;
use shared::build_replay_fixture_draft;
use uuid::Uuid;

use crate::{
    bootstrap::CliState,
    render::{print_json, print_timeline},
};

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Create a new session for the given scenario.
    Create {
        #[arg(long)]
        scenario: Uuid,
        /// Optional session title. Defaults to "CLI Session".
        #[arg(long)]
        title: Option<String>,
    },
    /// List all sessions.
    List,
    /// Get a session by id.
    Get { session_id: Uuid },
    /// Assign or clear the provider used for a session.
    SetProvider {
        session_id: Uuid,
        /// Provider id from `rp provider list`. Mutually exclusive with --clear.
        #[arg(long, conflicts_with = "clear")]
        provider_id: Option<Uuid>,
        /// Clear the session's provider override, falling back to the default.
        #[arg(long, conflicts_with = "provider_id")]
        clear: bool,
    },
    /// Show the ordered timeline for a session.
    Timeline {
        session_id: Uuid,
        /// Print raw/admin timeline data as JSON.
        #[arg(long)]
        admin: bool,
    },
    /// Show the effective provider mode for a session.
    Provider { session_id: Uuid },
    /// Export a replay fixture draft for a session.
    ExportFixture {
        session_id: Uuid,
        #[arg(long)]
        name: String,
    },
}

pub async fn run(state: CliState, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Create { scenario, title } => {
            let title = title.unwrap_or_else(|| "CLI Session".into());
            let session = state
                .store
                .create_session(scenario, title)
                .await?
                .ok_or_else(|| anyhow::anyhow!("scenario {scenario} not found"))?;
            print_json(&session)
        }
        Cmd::List => {
            let sessions = state.store.list_sessions().await?;
            print_json(&sessions)
        }
        Cmd::Get { session_id } => match state.store.get_session(session_id).await? {
            Some(session) => print_json(&session),
            None => anyhow::bail!("session {session_id} not found"),
        },
        Cmd::SetProvider {
            session_id,
            provider_id,
            clear,
        } => {
            if !clear && provider_id.is_none() {
                anyhow::bail!("must provide --provider-id <UUID> or --clear");
            }
            let next = if clear { None } else { provider_id };
            let session = state
                .store
                .set_session_provider(session_id, next)
                .await?
                .ok_or_else(|| anyhow::anyhow!("session {session_id} not found"))?;
            print_json(&session)
        }
        Cmd::Timeline { session_id, admin } => {
            if admin {
                let raw_timeline = state
                    .store
                    .raw_timeline(session_id)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("session {session_id} not found"))?;
                print_json(&raw_timeline)
            } else {
                let timeline = state.store.timeline(session_id).await?;
                print_timeline(&timeline)
            }
        }
        Cmd::Provider { session_id } => {
            let session = state
                .store
                .get_session(session_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("session {session_id} not found"))?;
            print_session_provider(&session);
            Ok(())
        }
        Cmd::ExportFixture { session_id, name } => {
            let fixture = export_replay_fixture_draft(&state, session_id, name).await?;
            print_json(&fixture)
        }
    }
}

fn print_session_provider(session: &SessionRecord) {
    for line in describe_session_provider(session) {
        println!("{line}");
    }
}

fn describe_session_provider(session: &SessionRecord) -> Vec<String> {
    let mut lines = vec![format!("session: {}", session.id)];
    match session.provider_id {
        Some(provider_id) => {
            lines.push("provider mode: override".into());
            lines.push(format!("provider id: {provider_id}"));
        }
        None => {
            lines.push("provider mode: default".into());
            lines.push("provider id: (default)".into());
        }
    }
    lines
}

async fn export_replay_fixture_draft(
    state: &CliState,
    session_id: Uuid,
    name: String,
) -> Result<shared::ReplayFixture> {
    let session = state
        .store
        .get_session(session_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("session {session_id} not found"))?;
    let scenario = state
        .store
        .get_scenario(session.scenario_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("scenario {} not found", session.scenario_id))?;
    let world_state = state
        .store
        .world_state(session_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("world state for session {session_id} not found"))?;
    let visible_state = project_visible_state(&scenario, &world_state);

    Ok(build_replay_fixture_draft(
        name,
        Some(session_id),
        scenario,
        &world_state,
        &visible_state,
    ))
}

fn project_visible_state(
    scenario: &domain::Scenario,
    world_state: &WorldState,
) -> domain::FrontendVisibleState {
    engine::BasicFrontendStateProjector.project(scenario, world_state, &ViewerContext::player())
}

#[cfg(test)]
mod tests {
    use super::{describe_session_provider, project_visible_state};
    use domain::{Fact, FactSource, FactVisibility, FrontendVisibleState, Scenario, ScenarioType};
    use persistence::SessionRecord;
    use shared::build_replay_fixture_draft;
    use uuid::Uuid;

    #[test]
    fn describe_session_provider_reports_default_mode() {
        let session = SessionRecord {
            id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            title: "test".into(),
            status: "active".into(),
            provider_id: None,
        };

        assert_eq!(
            describe_session_provider(&session),
            vec![
                format!("session: {}", session.id),
                "provider mode: default".to_string(),
                "provider id: (default)".to_string(),
            ]
        );
    }

    #[test]
    fn describe_session_provider_reports_override_mode() {
        let provider_id = Uuid::new_v4();
        let session = SessionRecord {
            id: Uuid::new_v4(),
            scenario_id: Uuid::new_v4(),
            title: "test".into(),
            status: "active".into(),
            provider_id: Some(provider_id),
        };

        assert_eq!(
            describe_session_provider(&session),
            vec![
                format!("session: {}", session.id),
                "provider mode: override".to_string(),
                format!("provider id: {provider_id}"),
            ]
        );
    }

    fn scenario() -> Scenario {
        Scenario {
            id: Uuid::new_v4(),
            title: "fixture export".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "test".into(),
            tone: "test".into(),
            rules: vec![],
            locations: vec![],
            factions: vec![],
            npcs: vec![],
            quests: vec![],
            secrets: vec![],
            clocks: vec![],
        }
    }

    #[test]
    fn replay_fixture_draft_uses_shared_schema() {
        let scenario = scenario();
        let world_state = domain::WorldState {
            session_id: Uuid::new_v4(),
            scenario_id: scenario.id,
            version: 2,
            current_location_id: None,
            current_scene: None,
            active_speaker_id: None,
            facts: vec![
                Fact {
                    id: "player-fact".into(),
                    text: "The player is trusted.".into(),
                    visibility: FactVisibility::PlayerKnown,
                    known_by: vec![],
                    source: FactSource::Turn,
                    reveal_conditions: vec![],
                    related_secret_ids: vec![],
                    reveal_condition_satisfied: None,
                },
                Fact {
                    id: "gm-secret".into(),
                    text: "Hidden truth".into(),
                    visibility: FactVisibility::GmOnly,
                    known_by: vec![],
                    source: FactSource::Turn,
                    reveal_conditions: vec![],
                    related_secret_ids: vec![],
                    reveal_condition_satisfied: None,
                },
            ],
            npcs: vec![],
            factions: vec![],
            quests: vec![],
            clocks: vec![],
            relationships: vec![],
            inventory: vec![],
            memories: vec![],
            summary: None,
            recent_events: vec![],
        };
        let visible_state: FrontendVisibleState = project_visible_state(&scenario, &world_state);

        let fixture = build_replay_fixture_draft(
            "draft".into(),
            Some(world_state.session_id),
            scenario.clone(),
            &world_state,
            &visible_state,
        );

        assert_eq!(fixture.version, 1);
        assert_eq!(fixture.name, "draft");
        assert_eq!(fixture.source_session_id, Some(world_state.session_id));
        assert_eq!(fixture.scenario, scenario);
        assert!(fixture.turns.is_empty());
        assert_eq!(fixture.expected_final.world_state_version, 2);
        assert_eq!(
            fixture.expected_final.hidden_fact_ids_absent_from_projection,
            vec!["gm-secret".to_string()]
        );
    }
}
