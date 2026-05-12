//! Small rendering helpers used by the subcommand handlers.

use std::{io::Write, sync::Arc};

use anyhow::Result;
use domain::{TurnMode, ViewerContext};
use engine::{
    DefaultTurnPipeline, SessionTurnLock, StreamTurnEvent, StreamTurnRequest, TurnStateStore,
    stream_turn,
};
use futures::StreamExt;
use providers::LlmProvider;
use serde::Serialize;
use uuid::Uuid;

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let rendered = serde_json::to_string_pretty(value)?;
    println!("{rendered}");
    Ok(())
}

/// Streams a turn through [`engine::stream_turn`] and renders it to stdout:
/// tokens live, then a `---` separator, then a one-line `world_state_version`,
/// `changed_entities` (JSON), and optional `usage`/`cost_usd` summary. Used by
/// both `rp turn --stream` and the `rp chat` REPL so output is identical.
pub async fn render_streaming_turn<P, S, L>(
    pipeline: Arc<DefaultTurnPipeline<P, S, L>>,
    session_id: Uuid,
    input: String,
    mode: Option<TurnMode>,
    viewer: ViewerContext,
) -> Result<()>
where
    P: ?Sized + LlmProvider + Send + Sync + 'static,
    S: ?Sized + TurnStateStore + Send + Sync + 'static,
    L: SessionTurnLock + Send + Sync + 'static,
{
    let stream = stream_turn(
        pipeline,
        StreamTurnRequest {
            session_id,
            input,
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
    write_final_summary(&mut handle, final_event, metadata.as_ref())?;
    Ok(())
}

fn write_final_summary(
    handle: &mut std::io::StdoutLock<'_>,
    final_event: Option<engine::StreamTurnFinal>,
    metadata: Option<&providers::StreamMetadata>,
) -> Result<()> {
    writeln!(handle)?;
    writeln!(handle, "---")?;
    if let Some(final_) = final_event {
        writeln!(handle, "world_state_version: {}", final_.world_state_version)?;
        writeln!(
            handle,
            "changed_entities: {}",
            serde_json::to_string(&final_.changed_entities)?
        )?;
        if let Some(usage) = metadata.and_then(|m| m.usage.as_ref()) {
            writeln!(
                handle,
                "usage: prompt={} completion={} total={}",
                usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
            )?;
        }
        if let Some(cost) = metadata.and_then(|m| m.cost_usd) {
            writeln!(handle, "cost_usd: {cost}")?;
        }
    }
    Ok(())
}

