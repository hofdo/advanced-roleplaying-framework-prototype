//! Engine-owned streaming turn orchestration.
//!
//! `stream_turn` runs the full streaming pipeline: acquires the session turn
//! lock, prepares context, calls the provider's streaming endpoint, filters
//! hidden reasoning markers, performs the second-pass delta extraction,
//! finalizes the validated delta into a new world state, and persists the turn.
//!
//! Callers consume a stream of [`StreamTurnEvent`] values. The HTTP API maps
//! these to SSE events; the CLI binary renders them directly to stdout. Errors
//! surface as `Err(TurnPipelineError)` and terminate the stream.

use std::sync::Arc;

use async_stream::try_stream;
use domain::{EntityRef, FrontendStatePatch, SessionId, TurnMode, ViewerContext};
use futures::{Stream, StreamExt};
use providers::{LlmProvider, ProviderError, ProviderStreamEvent, StreamMetadata, TokenUsage};
use uuid::Uuid;

use crate::{
    DefaultTurnPipeline, HiddenReasoningStripper, PipelineEventKind, PromptBuilder,
    SessionTurnLock, TurnPipelineError, TurnStateStore,
};

#[derive(Debug, Clone)]
pub struct StreamTurnRequest {
    pub session_id: SessionId,
    pub input: String,
    pub mode: Option<TurnMode>,
    pub viewer: ViewerContext,
}

#[derive(Debug, Clone)]
pub enum StreamTurnEvent {
    /// Visible narration token. Already filtered for hidden-reasoning markers.
    Token(String),
    /// Out-of-band provider metadata (usage, cost, generation id). Emitted at
    /// most once during the stream, before [`StreamTurnEvent::Final`].
    ProviderMetadata(StreamMetadata),
    /// Terminal event: the validated delta has been applied and persisted.
    Final(StreamTurnFinal),
}

#[derive(Debug, Clone)]
pub struct StreamTurnFinal {
    pub message_id: Uuid,
    pub world_state_version: i64,
    pub changed_entities: Vec<EntityRef>,
    pub frontend_state_patch: FrontendStatePatch,
    pub provider_usage: Option<TokenUsage>,
    pub provider_cost_usd: Option<f64>,
    pub generation_id: Option<String>,
}

/// Drives the full streaming turn pipeline and yields [`StreamTurnEvent`]s.
///
/// The pipeline is cloned (`Arc`) into the stream so the returned stream is
/// `'static` and can outlive the call site. The turn lock acquired internally
/// is released when the stream completes or is dropped.
pub fn stream_turn<P, S, L>(
    pipeline: Arc<DefaultTurnPipeline<P, S, L>>,
    request: StreamTurnRequest,
) -> impl Stream<Item = Result<StreamTurnEvent, TurnPipelineError>> + Send + 'static
where
    P: ?Sized + LlmProvider + Send + Sync + 'static,
    S: ?Sized + TurnStateStore + Send + Sync + 'static,
    L: SessionTurnLock + Send + Sync + 'static,
{
    try_stream! {
        let session_id = request.session_id;
        let input = request.input;
        let mode = request.mode;
        let viewer = request.viewer;

        record_event(&pipeline, session_id, PipelineEventKind::TurnStarted).await?;
        let _guard = pipeline.turn_lock.acquire(session_id).await?;
        record_event(&pipeline, session_id, PipelineEventKind::TurnLockAcquired).await?;

        // --- Preparation: load state, classify scene, build context ---
        let prepared = pipeline
            .prepare_turn_context(session_id, &input, mode)
            .await?;
        record_event(&pipeline, session_id, PipelineEventKind::ContextBuilt).await?;

        // --- Streaming provider call: emits visible narration tokens only ---
        record_event(&pipeline, session_id, PipelineEventKind::ProviderCalled).await?;
        let streaming_prompt = pipeline
            .prompt_builder
            .build_streaming_prompt(&prepared.context, &input);
        let token_stream = match pipeline.provider.stream(streaming_prompt).await {
            Ok(stream) => {
                record_event(&pipeline, session_id, PipelineEventKind::ProviderResponded).await?;
                stream
            }
            Err(error) => {
                yield_err(&pipeline, session_id, &error).await;
                Err(TurnPipelineError::Provider(error))?;
                unreachable!()
            }
        };

        futures::pin_mut!(token_stream);
        let mut raw_response = String::new();
        let mut stream_meta: Option<StreamMetadata> = None;
        while let Some(token) = token_stream.next().await {
            match token {
                Ok(ProviderStreamEvent::Token(token)) => {
                    if is_reasoning_marker(&token) {
                        continue;
                    }
                    raw_response.push_str(&token);
                    yield StreamTurnEvent::Token(token);
                }
                Ok(ProviderStreamEvent::Metadata(meta)) => {
                    stream_meta = Some(meta.clone());
                    yield StreamTurnEvent::ProviderMetadata(meta);
                }
                Err(error) => {
                    yield_err(&pipeline, session_id, &error).await;
                    Err(TurnPipelineError::Provider(error))?;
                    unreachable!()
                }
            }
        }

        // --- Strip hidden reasoning, then second-pass delta extraction ---
        let visible_response = pipeline.stripper.strip(&raw_response);
        let delta_prompt = pipeline.prompt_builder.build_delta_extraction_prompt(
            &prepared.context,
            &input,
            &visible_response,
        );
        let delta_response = match pipeline.provider.generate(delta_prompt).await {
            Ok(response) => response,
            Err(error) => {
                let description = error.to_string();
                let _ = pipeline.store.persist_error_event(session_id, description).await;
                Err(TurnPipelineError::Provider(error))?;
                unreachable!()
            }
        };

        // --- Finalization: validate, reduce, project, build records ---
        let mut finalized = pipeline
            .finalize_turn_delta(
                session_id,
                &prepared,
                &visible_response,
                &delta_response.text,
                &input,
                &viewer,
            )
            .await?;
        finalized.assistant_message.raw_provider_output = Some(serde_json::json!({
            "visible_response_text": visible_response,
            "delta_raw_output": delta_response
                .raw_json
                .clone()
                .unwrap_or_else(|| serde_json::Value::String(delta_response.text.clone())),
            "provider_usage": stream_meta.as_ref().and_then(|m| m.usage.as_ref()),
            "provider_cost_usd": stream_meta.as_ref().and_then(|m| m.cost_usd),
            "generation_id": stream_meta.as_ref().and_then(|m| m.generation_id.as_ref()),
        }));

        if let Some(ref meta) = stream_meta {
            let description = serde_json::json!({
                "usage": meta.usage,
                "cost_usd": meta.cost_usd,
            })
            .to_string();
            let _ = pipeline
                .store
                .persist_pipeline_event(
                    session_id,
                    PipelineEventKind::ProviderUsageCaptured.as_str(),
                    description,
                )
                .await;
        }
        record_event(&pipeline, session_id, PipelineEventKind::DeltaApplied).await?;
        record_event(
            &pipeline,
            session_id,
            PipelineEventKind::FrontendStateProjected,
        )
        .await?;

        let message_id = finalized.assistant_message.id;
        let world_state_version = finalized.world_state_version;
        let changed_entities = finalized.frontend_state_patch.changed_entities.clone();
        let frontend_state_patch = finalized.frontend_state_patch.clone();

        pipeline
            .store
            .persist_successful_turn(
                finalized.user_message,
                finalized.assistant_message,
                finalized.validated_delta,
                finalized.updated_world_state,
            )
            .await?;
        record_event(&pipeline, session_id, PipelineEventKind::TurnFinished).await?;
        record_event(&pipeline, session_id, PipelineEventKind::TurnLockReleasing).await?;

        let provider_usage = stream_meta.as_ref().and_then(|m| m.usage.clone());
        let provider_cost_usd = stream_meta.as_ref().and_then(|m| m.cost_usd);
        let generation_id = stream_meta.as_ref().and_then(|m| m.generation_id.clone());

        yield StreamTurnEvent::Final(StreamTurnFinal {
            message_id,
            world_state_version,
            changed_entities,
            frontend_state_patch,
            provider_usage,
            provider_cost_usd,
            generation_id,
        });
    }
}

/// Mirrors `app.rs`'s ad-hoc reasoning-marker filter so streaming output stays
/// free of leaked hidden-reasoning prefixes even when the provider does not
/// honor the streaming prompt's reasoning constraints.
fn is_reasoning_marker(token: &str) -> bool {
    token.contains("<think>")
        || token.contains("Internal reasoning:")
        || token.contains("Chain of thought:")
        || token.contains("Hidden reasoning:")
        || token.contains("GM reasoning:")
}

async fn record_event<P, S, L>(
    pipeline: &DefaultTurnPipeline<P, S, L>,
    session_id: SessionId,
    event: PipelineEventKind,
) -> Result<(), TurnPipelineError>
where
    P: ?Sized,
    S: ?Sized + TurnStateStore,
    L: SessionTurnLock,
{
    pipeline
        .store
        .persist_pipeline_event(session_id, event.as_str(), event.as_str().to_owned())
        .await
}

async fn yield_err<P, S, L>(
    pipeline: &DefaultTurnPipeline<P, S, L>,
    session_id: SessionId,
    error: &ProviderError,
) where
    P: ?Sized,
    S: ?Sized + TurnStateStore,
    L: SessionTurnLock,
{
    if matches!(error, ProviderError::StreamIdleTimeout) {
        let _ = pipeline
            .store
            .persist_pipeline_event(
                session_id,
                "provider_stream_idle_timeout",
                "provider_stream_idle_timeout".into(),
            )
            .await;
    }
}
