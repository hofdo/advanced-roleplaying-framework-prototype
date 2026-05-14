use anyhow::Result;
use clap::Subcommand;
use domain::Scenario;
use uuid::Uuid;

use crate::{
    bootstrap::CliState,
    render::print_json,
    samples::{build_sample, sample_names},
    scenario_io::read_scenario_file,
};

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Create a scenario from JSON on disk or a built-in sample.
    Create {
        /// Path to a scenario JSON file.
        #[arg(long, conflicts_with = "sample")]
        file: Option<String>,
        /// Name of a built-in sample scenario.
        #[arg(long, conflicts_with = "file")]
        sample: Option<String>,
    },
    /// Validate a scenario JSON file without persisting it.
    Validate {
        /// Path to a scenario JSON file.
        #[arg(long)]
        file: String,
    },
    /// List all scenarios.
    List,
    /// Get a scenario by id.
    Get { scenario_id: Uuid },
    /// Delete a scenario by id.
    Delete { scenario_id: Uuid },
}

pub async fn run(state: CliState, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Create { file, sample } => create(state, file, sample).await,
        Cmd::Validate { file } => validate(&file),
        Cmd::List => {
            let scenarios = state.store.list_scenarios().await?;
            print_json(&scenarios)
        }
        Cmd::Get { scenario_id } => match state.store.get_scenario(scenario_id).await? {
            Some(scenario) => print_json(&scenario),
            None => anyhow::bail!("scenario {scenario_id} not found"),
        },
        Cmd::Delete { scenario_id } => {
            let deleted = state.store.delete_scenario(scenario_id).await?;
            if !deleted {
                anyhow::bail!("scenario {scenario_id} not found");
            }
            println!("deleted scenario {scenario_id}");
            Ok(())
        }
    }
}

async fn create(state: CliState, file: Option<String>, sample: Option<String>) -> Result<()> {
    let scenario: Scenario = match (file, sample) {
        (Some(path), None) => read_scenario_file(&path)?,
        (None, Some(name)) => build_sample(&name)?,
        (None, None) => anyhow::bail!(
            "must provide --file <PATH> or --sample <NAME>; known samples: {}",
            sample_names().join(", ")
        ),
        (Some(_), Some(_)) => unreachable!("clap enforces conflicts_with"),
    };
    let created = state.store.create_scenario(scenario).await?;
    print_json(&created)
}

fn validate(path: &str) -> Result<()> {
    let scenario = read_scenario_file(path)?;
    println!("valid scenario:");
    println!("title: {}", scenario.title);
    println!("locations: {}", scenario.locations.len());
    println!("npcs: {}", scenario.npcs.len());
    println!("factions: {}", scenario.factions.len());
    println!("quests: {}", scenario.quests.len());
    println!("secrets: {}", scenario.secrets.len());
    println!("clocks: {}", scenario.clocks.len());
    Ok(())
}
