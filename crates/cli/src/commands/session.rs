use anyhow::Result;
use clap::Subcommand;
use uuid::Uuid;

use crate::{bootstrap::CliState, render::print_json};

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
    }
}
