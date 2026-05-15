use anyhow::Result;
use clap::Subcommand;
use persistence::SessionRecord;
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

#[cfg(test)]
mod tests {
    use super::describe_session_provider;
    use persistence::SessionRecord;
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
}
