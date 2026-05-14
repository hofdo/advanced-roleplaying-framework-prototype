use anyhow::Result;
use clap::Args as ClapArgs;
use domain::ViewerContext;
use engine::{BasicFrontendStateProjector, FrontendStateProjector};
use uuid::Uuid;

use crate::{bootstrap::CliState, render::print_json};

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub session_id: Uuid,
    /// Use admin viewer context (returns the raw, unfiltered world state).
    #[arg(long)]
    pub admin: bool,
}

pub async fn run(state: CliState, args: Args) -> Result<()> {
    let Some(session) = state.store.get_session(args.session_id).await? else {
        anyhow::bail!("session {} not found", args.session_id);
    };
    let Some(world_state) = state.store.world_state(args.session_id).await? else {
        anyhow::bail!("world state for session {} not found", args.session_id);
    };

    if args.admin {
        return print_json(&world_state);
    }

    let Some(scenario) = state.store.get_scenario(session.scenario_id).await? else {
        anyhow::bail!("scenario {} not found", session.scenario_id);
    };
    let projected =
        BasicFrontendStateProjector.project(&scenario, &world_state, &ViewerContext::player());
    print_json(&projected)
}
