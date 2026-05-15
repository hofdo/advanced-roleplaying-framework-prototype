//! Small rendering helpers used by the subcommand handlers.

use std::{io::Write, sync::Arc};

use anyhow::Result;
use clap::ValueEnum;
use domain::{TurnMode, ViewerContext};
use engine::{
    DefaultTurnPipeline, SessionTurnLock, StreamTurnEvent, StreamTurnFinal, StreamTurnRequest,
    TurnResponse, TurnStateStore, stream_turn,
};
use futures::StreamExt;
use persistence::TimelineEntry;
use providers::{LlmProvider, StreamMetadata};
use serde::Serialize;
use uuid::Uuid;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputView {
    #[default]
    Verbose,
    Quiet,
}

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let rendered = serde_json::to_string_pretty(value)?;
    println!("{rendered}");
    Ok(())
}

pub fn print_timeline(entries: &[TimelineEntry]) -> Result<()> {
    for line in format_timeline_lines(entries) {
        println!("{line}");
    }
    Ok(())
}

pub fn print_turn_response(response: &TurnResponse, view: OutputView) -> Result<()> {
    match view {
        OutputView::Verbose => print_json(&serde_json::json!({
            "message_id": response.message_id,
            "player_response": response.player_response,
            "scene_type": response.scene_type,
            "world_state_version": response.world_state_version,
            "changed_entities": response.changed_entities,
            "frontend_state_patch": response.frontend_state_patch,
        })),
        OutputView::Quiet => print_narrative(&response.player_response),
    }
}

pub fn print_narrative(text: &str) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    write_narrative_block(&mut handle, text)
}

/// Streams a turn through [`engine::stream_turn`] and renders it to stdout.
pub async fn render_streaming_turn<P, S, L>(
    pipeline: Arc<DefaultTurnPipeline<P, S, L>>,
    session_id: Uuid,
    input: String,
    mode: Option<TurnMode>,
    viewer: ViewerContext,
    view: OutputView,
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
    write_final_summary(&mut handle, view, final_event, metadata.as_ref())?;
    Ok(())
}

fn write_narrative_block<W: Write>(writer: &mut W, text: &str) -> Result<()> {
    writeln!(writer, "{}", format_narrative_text(text))?;
    Ok(())
}

fn write_final_summary<W: Write>(
    handle: &mut W,
    view: OutputView,
    final_event: Option<StreamTurnFinal>,
    metadata: Option<&StreamMetadata>,
) -> Result<()> {
    writeln!(handle)?;
    if matches!(view, OutputView::Quiet) {
        return Ok(());
    }

    writeln!(handle, "---")?;
    if let Some(final_) = final_event {
        writeln!(
            handle,
            "world_state_version: {}",
            final_.world_state_version
        )?;
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

fn format_narrative_text(text: &str) -> String {
    text.trim()
        .split("\n\n")
        .filter_map(|section| {
            let lines = section
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            if lines.is_empty() {
                None
            } else if lines.len() == 1 {
                Some(wrap_paragraph(lines[0], 78))
            } else {
                Some(format!(
                    "{}\n{}",
                    lines[0],
                    wrap_paragraph(&lines[1..].join(" "), 78)
                ))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn wrap_paragraph(text: &str, width: usize) -> String {
    let words = text.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return String::new();
    }

    let mut current = String::new();
    let mut lines = Vec::new();
    for word in words {
        let next_len = if current.is_empty() {
            word.len()
        } else {
            current.len() + 1 + word.len()
        };
        if next_len > width && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
        } else if current.is_empty() {
            current = word.to_string();
        } else {
            current.push(' ');
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines.join("\n")
}

fn format_timeline_lines(entries: &[TimelineEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| {
            let version = entry
                .world_state_version
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".into());
            format!("{} {} {}", entry.kind, version, entry.description)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{OutputView, format_narrative_text, format_timeline_lines, write_final_summary};
    use domain::FrontendStatePatch;
    use engine::StreamTurnFinal;
    use persistence::TimelineEntry;
    use providers::{StreamMetadata, TokenUsage};
    use uuid::Uuid;

    #[test]
    fn format_timeline_lines_prints_kind_version_and_description() {
        let entries = vec![
            TimelineEntry {
                kind: "user_message".into(),
                description: "I answer plainly.".into(),
                message_id: Some(Uuid::new_v4()),
                event_id: None,
                world_state_version: None,
            },
            TimelineEntry {
                kind: "world_event".into(),
                description: "The ledger is updated.".into(),
                message_id: None,
                event_id: Some(Uuid::new_v4()),
                world_state_version: Some(3),
            },
        ];

        assert_eq!(
            format_timeline_lines(&entries),
            vec![
                "user_message - I answer plainly.".to_string(),
                "world_event 3 The ledger is updated.".to_string(),
            ]
        );
    }

    #[test]
    fn quiet_narrative_formatting_outputs_only_wrapped_text() {
        let rendered = format_narrative_text(
            "Opening\nYou begin in a hall with enough detail to require wrapping across multiple words for this test."
        );

        assert!(rendered.starts_with("Opening\n"));
        assert!(!rendered.contains("world_state_version"));
        assert!(rendered.contains('\n'));
    }

    #[test]
    fn quiet_streaming_summary_suppresses_metadata_block() {
        let mut out = Vec::new();
        let final_event = StreamTurnFinal {
            message_id: Uuid::new_v4(),
            world_state_version: 2,
            changed_entities: vec![],
            frontend_state_patch: FrontendStatePatch {
                state_version: 2,
                changed_entities: vec![],
                visible_state: None,
            },
            provider_usage: None,
            provider_cost_usd: None,
            generation_id: None,
        };
        let metadata = StreamMetadata {
            generation_id: None,
            usage: Some(TokenUsage {
                prompt_tokens: 1,
                completion_tokens: 2,
                total_tokens: 3,
            }),
            cost_usd: Some(0.01),
            extra: serde_json::Value::Null,
        };

        write_final_summary(&mut out, OutputView::Quiet, Some(final_event), Some(&metadata))
            .expect("summary should render");
        assert_eq!(String::from_utf8(out).expect("utf-8"), "\n");
    }

    #[test]
    fn verbose_streaming_summary_keeps_metadata_block() {
        let mut out = Vec::new();
        let final_event = StreamTurnFinal {
            message_id: Uuid::new_v4(),
            world_state_version: 2,
            changed_entities: vec![],
            frontend_state_patch: FrontendStatePatch {
                state_version: 2,
                changed_entities: vec![],
                visible_state: None,
            },
            provider_usage: None,
            provider_cost_usd: None,
            generation_id: None,
        };

        write_final_summary(&mut out, OutputView::Verbose, Some(final_event), None)
            .expect("summary should render");
        let rendered = String::from_utf8(out).expect("utf-8");
        assert!(rendered.contains("---"));
        assert!(rendered.contains("world_state_version: 2"));
    }
}
