use std::{io::Write, sync::Arc};

use anyhow::Result;
use clap::{Args as ClapArgs, ValueEnum};
use domain::{TurnMode, ViewerContext};
use engine::{DefaultTurnPipeline, StreamTurnEvent, StreamTurnRequest, TurnRequestInput};
use futures::StreamExt;
use uuid::Uuid;

use crate::{bootstrap::CliState, render::print_json};

#[derive(ClapArgs, Debug)]
pub struct Args {
    pub session_id: Uuid,
    /// Player input for this turn.
    #[arg(long)]
    pub input: String,
    /// Optional override of the engine's scene classifier.
    #[arg(long, value_enum)]
    pub mode: Option<Mode>,
    /// Stream visible narration tokens to stdout as they arrive.
    #[arg(long)]
    pub stream: bool,
    /// Use admin viewer context (sees GM-only facts in projections).
    #[arg(long)]
    pub admin: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Mode {
    Action,
    Dialogue,
    Direct,
    Remember,
}

impl From<Mode> for TurnMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Action => TurnMode::Action,
            Mode::Dialogue => TurnMode::Dialogue,
            Mode::Direct => TurnMode::Direct,
            Mode::Remember => TurnMode::Remember,
        }
    }
}

pub async fn run(state: CliState, args: Args) -> Result<()> {
    let viewer = if args.admin {
        ViewerContext {
            include_debug_state: true,
            is_admin: true,
        }
    } else {
        ViewerContext::player()
    };
    let mode: Option<TurnMode> = args.mode.map(Into::into);

    let pipeline = Arc::new(DefaultTurnPipeline::with_lock(
        Arc::clone(&state.provider),
        Arc::clone(&state.store),
        state.turn_lock.clone(),
    ));

    if args.stream {
        let stream = engine::stream_turn(
            pipeline,
            StreamTurnRequest {
                session_id: args.session_id,
                input: args.input,
                mode,
                viewer,
            },
        );
        futures::pin_mut!(stream);

        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        let mut metadata = None;
        let mut final_event = None;

        while let Some(event) = stream.next().await {
            match event? {
                StreamTurnEvent::Token(token) => {
                    handle.write_all(token.as_bytes())?;
                    handle.flush()?;
                }
                StreamTurnEvent::ProviderMetadata(meta) => {
                    metadata = Some(meta);
                }
                StreamTurnEvent::Final(final_) => {
                    final_event = Some(final_);
                }
            }
        }
        writeln!(handle)?;
        writeln!(handle, "---")?;

        if let Some(final_) = final_event {
            writeln!(handle, "world_state_version: {}", final_.world_state_version)?;
            writeln!(
                handle,
                "changed_entities: {}",
                serde_json::to_string(&final_.changed_entities)?
            )?;
            if let Some(usage) = metadata.as_ref().and_then(|m| m.usage.as_ref()) {
                writeln!(
                    handle,
                    "usage: prompt={} completion={} total={}",
                    usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
                )?;
            }
            if let Some(cost) = metadata.as_ref().and_then(|m| m.cost_usd) {
                writeln!(handle, "cost_usd: {cost}")?;
            }
        }
        Ok(())
    } else {
        let response = pipeline
            .process_turn(TurnRequestInput {
                session_id: args.session_id,
                input: args.input,
                mode,
                viewer,
            })
            .await?;
        print_json(&serde_json::json!({
            "message_id": response.message_id,
            "player_response": response.player_response,
            "scene_type": response.scene_type,
            "world_state_version": response.world_state_version,
            "changed_entities": response.changed_entities,
            "frontend_state_patch": response.frontend_state_patch,
        }))
    }
}
