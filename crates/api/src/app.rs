use crate::{ApiError, AppState, project_session_state};
use axum::{
    Json, Router,
    extract::{Path, State},
    response::sse::{Event, Sse},
    routing::{get, patch, post},
};
use domain::{Scenario, SessionId, TurnMode, ViewerContext};
use engine::{
    BasicFrontendStateProjector, DefaultTurnPipeline, FrontendStateProjector,
    HiddenReasoningStripper, PromptBuilder, SessionTurnLock, TurnRequestInput,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, sync::Arc};
use uuid::Uuid;

pub fn app_router(app_state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/providers", get(list_providers))
        .route("/providers/test", post(test_provider))
        .route("/providers/health", get(provider_health))
        .route("/providers/readiness", get(provider_readiness))
        .route(
            "/sessions/:session_id/provider",
            patch(set_session_provider),
        )
        .route("/scenarios", post(create_scenario).get(list_scenarios))
        .route(
            "/scenarios/:scenario_id",
            get(get_scenario)
                .put(update_scenario)
                .delete(delete_scenario),
        )
        .route("/sessions", post(create_session).get(list_sessions))
        .route(
            "/sessions/:session_id",
            get(get_session).delete(delete_session),
        )
        .route("/sessions/:session_id/export", get(export_session))
        .route("/sessions/:session_id/turn", post(turn))
        .route("/sessions/:session_id/turn/stream", post(turn_stream))
        .route("/sessions/:session_id/world-state", get(get_world_state))
        .route("/sessions/:session_id/events", get(list_events))
        // Raw (admin) export — returns the full WorldState without projection.
        // TODO: guard with authentication before exposing in production.
        .route(
            "/admin/sessions/:session_id/export/raw",
            get(export_session_raw),
        )
        .with_state(app_state)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        active_provider: state.config.provider.default.name,
        database: state.store.storage_status().await,
    })
}

async fn list_providers(State(state): State<AppState>) -> Json<Vec<ProviderResponse>> {
    Json(vec![ProviderResponse {
        name: state.config.provider.default.name,
        provider_type: state.config.provider.default.provider_type,
        base_url: state.config.provider.default.base_url,
        model: state.config.provider.default.model,
        supports_streaming: state.config.provider.default.supports_streaming,
        supports_json_mode: state.config.provider.default.supports_json_mode,
    }])
}

async fn test_provider(
    State(state): State<AppState>,
) -> Result<Json<ProviderTestResponse>, ApiError> {
    let health = state
        .provider
        .health()
        .await
        .map_err(engine::TurnPipelineError::from)?;
    Ok(Json(ProviderTestResponse {
        ok: health.ok,
        message: health.message.unwrap_or_else(|| "configured".into()),
    }))
}

async fn provider_health(
    State(state): State<AppState>,
) -> Result<Json<ProviderHealthResponse>, ApiError> {
    let health = state
        .provider
        .health()
        .await
        .map_err(engine::TurnPipelineError::from)?;
    Ok(Json(ProviderHealthResponse {
        name: health.name,
        ok: health.ok,
        message: health.message.unwrap_or_else(|| "configured".into()),
    }))
}

async fn provider_readiness(
    State(state): State<AppState>,
) -> Result<Json<ProviderReadinessResponse>, ApiError> {
    let readiness = state
        .provider
        .readiness()
        .await
        .map_err(engine::TurnPipelineError::from)?;
    Ok(Json(ProviderReadinessResponse {
        configured: readiness.configured,
        reachable: readiness.reachable,
        message: readiness.message,
    }))
}

async fn set_session_provider(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
    Json(request): Json<SetProviderRequest>,
) -> Result<Json<persistence::SessionRecord>, ApiError> {
    let provider_id = request.provider_id;
    state
        .store
        .set_session_provider(session_id, provider_id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn create_scenario(
    State(state): State<AppState>,
    Json(scenario): Json<Scenario>,
) -> Result<Json<Scenario>, ApiError> {
    domain::validate_scenario(&scenario)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(state.store.create_scenario(scenario).await?))
}

async fn list_scenarios(State(state): State<AppState>) -> Result<Json<Vec<Scenario>>, ApiError> {
    Ok(Json(state.store.list_scenarios().await?))
}

async fn get_scenario(
    State(state): State<AppState>,
    Path(scenario_id): Path<Uuid>,
) -> Result<Json<Scenario>, ApiError> {
    state
        .store
        .get_scenario(scenario_id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn update_scenario(
    State(state): State<AppState>,
    Path(scenario_id): Path<Uuid>,
    Json(mut scenario): Json<Scenario>,
) -> Result<Json<Scenario>, ApiError> {
    scenario.id = scenario_id;
    domain::validate_scenario(&scenario)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    state
        .store
        .update_scenario(scenario)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn delete_scenario(
    State(state): State<AppState>,
    Path(scenario_id): Path<Uuid>,
) -> Result<Json<DeleteResponse>, ApiError> {
    if state.store.delete_scenario(scenario_id).await? {
        Ok(Json(DeleteResponse { deleted: true }))
    } else {
        Err(ApiError::not_found())
    }
}

async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<persistence::SessionRecord>, ApiError> {
    state
        .store
        .create_session(request.scenario_id, request.title)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<persistence::SessionRecord>>, ApiError> {
    Ok(Json(state.store.list_sessions().await?))
}

async fn get_session(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<persistence::SessionRecord>, ApiError> {
    state
        .store
        .get_session(session_id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn delete_session(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<DeleteResponse>, ApiError> {
    if state.store.delete_session(session_id).await? {
        Ok(Json(DeleteResponse { deleted: true }))
    } else {
        Err(ApiError::not_found())
    }
}

async fn export_session(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<ExportSessionResponse>, ApiError> {
    let session = state
        .store
        .get_session(session_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let scenario = state
        .store
        .get_scenario(session.scenario_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let world_state = state
        .store
        .world_state(session_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let events = state.store.events(session_id).await?;
    // Project using player context so GM-only facts and hidden world state
    // are never exposed to the caller.
    let visible_state = BasicFrontendStateProjector.project(
        &scenario,
        &world_state,
        &ViewerContext::player(),
    );
    Ok(Json(ExportSessionResponse {
        session,
        visible_state,
        events,
    }))
}

async fn export_session_raw(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<RawExportSessionResponse>, ApiError> {
    // Returns full WorldState without projection. Intentionally unrestricted
    // for this local prototype — add authentication before production use.
    let session = state
        .store
        .get_session(session_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let world_state = state
        .store
        .world_state(session_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let events = state.store.events(session_id).await?;
    Ok(Json(RawExportSessionResponse {
        session,
        world_state,
        events,
    }))
}

async fn turn(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
    Json(request): Json<TurnRequest>,
) -> Result<Json<TurnResponseBody>, ApiError> {
    // Resolve provider: session-scoped override takes priority over default
    let session = state
        .store
        .get_session(session_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let provider = if session.provider_id.is_some() {
        // Session has a provider override — currently only the default provider
        // is instantiated, so we fall back to it. When a provider registry is
        // added this is where the lookup will go.
        Arc::clone(&state.provider)
    } else {
        Arc::clone(&state.provider)
    };
    let pipeline = DefaultTurnPipeline::with_lock(
        provider,
        Arc::clone(&state.store),
        state.turn_lock.clone(),
    );
    let response = pipeline
        .process_turn(TurnRequestInput {
            session_id,
            input: request.input,
            mode: request.mode,
            viewer: ViewerContext::player(),
        })
        .await?;
    Ok(Json(TurnResponseBody {
        message_id: response.message_id,
        player_response: response.player_response,
        scene_type: response.scene_type,
        world_state_version: response.world_state_version,
        changed_entities: response.changed_entities,
        frontend_state_patch: response.frontend_state_patch,
    }))
}

async fn turn_stream(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
    Json(request): Json<TurnRequest>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    // Resolve provider: session-scoped override takes priority over default.
    // Load session before entering the stream so we can pick the right provider.
    let resolved_provider = match state.store.get_session(session_id).await {
        Ok(Some(session)) => {
            if session.provider_id.is_some() {
                // Session has a provider override — currently only the default provider
                // is instantiated, so we fall back to it. When a provider registry is
                // added this is where the lookup will go.
                Arc::clone(&state.provider)
            } else {
                Arc::clone(&state.provider)
            }
        }
        Ok(None) => Arc::clone(&state.provider),
        Err(_) => Arc::clone(&state.provider),
    };

    let events = async_stream::stream! {
        let input = request.input;
        let mode = request.mode;
        // Build a pipeline so all component logic (prepare / finalize) lives in
        // the engine crate rather than being duplicated here.
        let pipeline = DefaultTurnPipeline::with_lock(
            Arc::clone(&resolved_provider),
            Arc::clone(&state.store),
            state.turn_lock.clone(),
        );

        let _guard = match pipeline.turn_lock.acquire(session_id).await {
            Ok(guard) => guard,
            Err(error) => {
                yield Ok(error_event(error.to_string()));
                return;
            }
        };

        // --- Preparation (lock, load, classify, context) ---
        let prepared = match pipeline.prepare_turn_context(session_id, &input, mode).await {
            Ok(prepared) => prepared,
            Err(error) => {
                yield Ok(error_event(error.to_string()));
                return;
            }
        };

        // --- Streaming (unique to this path) ---
        let token_stream = match resolved_provider
            .stream(pipeline.prompt_builder.build_streaming_prompt(&prepared.context, &input))
            .await
        {
            Ok(stream) => stream,
            Err(error) => {
                yield Ok(error_event(error.to_string()));
                return;
            }
        };

        futures::pin_mut!(token_stream);
        let mut raw_response = String::new();
        while let Some(token) = token_stream.next().await {
            match token {
                Ok(token) => {
                    if token.contains("<think>")
                        || token.contains("Internal reasoning:")
                        || token.contains("Chain of thought:")
                        || token.contains("Hidden reasoning:")
                        || token.contains("GM reasoning:")
                    {
                        continue;
                    }
                    raw_response.push_str(&token);
                    yield Ok(Event::default()
                        .event("token")
                        .json_data(TokenEvent { text: token })
                        .expect("token event serializes"));
                }
                Err(error) => {
                    yield Ok(error_event(error.to_string()));
                    return;
                }
            }
        }

        // Strip hidden reasoning from the accumulated tokens, then call the
        // provider a second time to extract a typed WorldStateDelta from the
        // narration (streaming path can't emit JSON inline).
        let visible_response = pipeline.stripper.strip(&raw_response);
        let delta_response = match resolved_provider
            .generate(pipeline.prompt_builder.build_delta_extraction_prompt(
                &prepared.context,
                &input,
                &visible_response,
            ))
            .await
        {
            Ok(response) => response,
            Err(error) => {
                let _ = pipeline.store.persist_error_event(session_id, error.to_string()).await;
                yield Ok(error_event(error.to_string()));
                return;
            }
        };

        // --- Finalization (validate, reduce, project, build message records) ---
        let finalized = match pipeline.finalize_turn_delta(
            session_id,
            &prepared,
            &visible_response,
            &delta_response.text,
            &input,
            &ViewerContext::player(),
        ).await {
            Ok(finalized) => finalized,
            Err(error) => {
                let _ = pipeline.store.persist_error_event(session_id, error.to_string()).await;
                yield Ok(error_event(error.to_string()));
                return;
            }
        };

        let message_id = finalized.assistant_message.id;
        let world_state_version = finalized.world_state_version;
        let frontend_state_patch = finalized.frontend_state_patch.clone();

        if let Err(error) = pipeline.store
            .persist_successful_turn(
                finalized.user_message,
                finalized.assistant_message,
                finalized.validated_delta,
                finalized.updated_world_state,
            )
            .await
        {
            yield Ok(error_event(error.to_string()));
            return;
        }

        yield Ok(Event::default()
            .event("final")
            .json_data(StreamFinalEvent {
                message_id,
                delta_applied: true,
                world_state_version,
                frontend_state_patch,
            })
            .expect("final event serializes"));
    };

    Sse::new(events)
}

async fn get_world_state(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<domain::FrontendVisibleState>, ApiError> {
    project_session_state(&state, session_id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn list_events(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
) -> Result<Json<Vec<persistence::EventRecord>>, ApiError> {
    Ok(Json(state.store.events(session_id).await?))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    active_provider: String,
    database: String,
}

#[derive(Debug, Serialize)]
struct ProviderResponse {
    name: String,
    provider_type: String,
    base_url: String,
    model: String,
    supports_streaming: bool,
    supports_json_mode: bool,
}

#[derive(Debug, Serialize)]
struct ProviderTestResponse {
    ok: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct ProviderHealthResponse {
    name: String,
    ok: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct ProviderReadinessResponse {
    configured: bool,
    reachable: bool,
    message: String,
}

#[derive(Debug, Deserialize)]
struct SetProviderRequest {
    provider_id: Option<Uuid>,
    provider_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    scenario_id: Uuid,
    title: String,
}

#[derive(Debug, Deserialize)]
struct TurnRequest {
    input: String,
    mode: Option<TurnMode>,
}

#[derive(Debug, Serialize)]
struct TurnResponseBody {
    message_id: Uuid,
    player_response: String,
    scene_type: domain::SceneReasoningStyle,
    world_state_version: i64,
    changed_entities: Vec<domain::EntityRef>,
    frontend_state_patch: domain::FrontendStatePatch,
}

#[derive(Debug, Serialize)]
struct DeleteResponse {
    deleted: bool,
}

#[derive(Debug, Serialize)]
struct ExportSessionResponse {
    session: persistence::SessionRecord,
    visible_state: domain::FrontendVisibleState,
    events: Vec<persistence::EventRecord>,
}

#[derive(Debug, Serialize)]
struct RawExportSessionResponse {
    session: persistence::SessionRecord,
    world_state: domain::WorldState,
    events: Vec<persistence::EventRecord>,
}

#[derive(Debug, Serialize)]
struct TokenEvent {
    text: String,
}

#[derive(Debug, Serialize)]
struct StreamFinalEvent {
    message_id: Uuid,
    delta_applied: bool,
    world_state_version: i64,
    frontend_state_patch: domain::FrontendStatePatch,
}

#[derive(Debug, Serialize)]
struct ErrorEvent {
    error: String,
}

fn error_event(error: String) -> Event {
    Event::default()
        .event("error")
        .json_data(ErrorEvent { error })
        .expect("error event serializes")
}
