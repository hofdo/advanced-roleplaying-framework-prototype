use crate::{ApiError, AppState, project_session_state, provider_from_record};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    response::sse::{Event, Sse},
    routing::{delete, get, patch, post},
};
use domain::{Scenario, SessionId, TurnMode, ViewerContext};
use engine::{
    BasicFrontendStateProjector, DefaultTurnPipeline, FrontendStateProjector, TurnRequestInput,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, sync::Arc};
use uuid::Uuid;

pub fn app_router(app_state: AppState) -> Router {
    let router = Router::new()
        .route("/health", get(health))
        .route("/providers", get(list_providers).post(register_provider))
        .route("/providers/test", post(test_provider))
        .route("/providers/health", get(provider_health))
        .route("/providers/readiness", get(provider_readiness))
        .route("/providers/:id", delete(delete_provider))
        .route("/providers/:id/models", get(list_provider_models))
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
        .route("/sessions/:session_id/events", get(list_events));

    let router = if app_state.config.admin.enabled {
        router.merge(
            Router::new()
                .route(
                    "/admin/sessions/:session_id/export/raw",
                    get(export_session_raw),
                )
                .route("/admin/sessions/:session_id/turn/debug", post(debug_turn))
                .route_layer(middleware::from_fn_with_state(
                    app_state.clone(),
                    require_admin_token,
                )),
        )
    } else {
        router
    };

    router.with_state(app_state)
}

async fn require_admin_token(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected_token) = state.config.admin.token.as_deref() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let Some(value) = request.headers().get(header::AUTHORIZATION) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let Ok(value) = value.to_str() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let Some(actual_token) = value.strip_prefix("Bearer ") else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if actual_token != expected_token {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        active_provider: state.config.provider.default.name,
        database: state.store.storage_status().await,
    })
}

async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<persistence::ProviderRecord>>, ApiError> {
    Ok(Json(state.store.list_providers().await?))
}

async fn register_provider(
    State(state): State<AppState>,
    Json(request): Json<RegisterProviderRequest>,
) -> Result<(StatusCode, Json<persistence::ProviderRecord>), ApiError> {
    let record = persistence::ProviderRecord {
        id: Uuid::new_v4(),
        name: request.name,
        provider_type: request.provider_type,
        base_url: request.base_url,
        model: request.model,
        api_key_secret_ref: request.api_key_secret_ref,
        capabilities: request
            .capabilities
            .unwrap_or(serde_json::Value::Object(Default::default())),
        is_default: request.is_default,
    };
    let provider =
        provider_from_record(&record).map_err(|error| ApiError::bad_request(error.to_string()))?;
    let created = state.store.create_provider(record.clone()).await?;
    state
        .provider_registry
        .write()
        .await
        .insert(record.id, provider);
    Ok((StatusCode::CREATED, Json(created)))
}

async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DeleteResponse>, ApiError> {
    state.store.delete_provider(id).await?;
    state.provider_registry.write().await.remove(&id);
    Ok(Json(DeleteResponse { deleted: true }))
}

async fn list_provider_models(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<providers::ProviderModel>>, ApiError> {
    let registry = state.provider_registry.read().await;
    let provider = registry.get(&id).cloned().ok_or_else(ApiError::not_found)?;
    drop(registry);
    provider.list_models().await.map(Json).map_err(|e| match e {
        providers::ProviderError::Unsupported(_) => {
            ApiError::status(StatusCode::NOT_IMPLEMENTED, e.to_string())
        }
        other => ApiError::from(engine::TurnPipelineError::from(other)),
    })
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
    let visible_state =
        BasicFrontendStateProjector.project(&scenario, &world_state, &ViewerContext::player());
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
    let provider = state
        .resolve_provider(session.provider_id)
        .await
        .map_err(|error| ApiError::status(StatusCode::CONFLICT, error.to_string()))?;
    let pipeline =
        DefaultTurnPipeline::with_lock(provider, Arc::clone(&state.store), state.turn_lock.clone());
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

async fn debug_turn(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
    Json(request): Json<TurnRequest>,
) -> Result<Json<DebugTurnResponseBody>, ApiError> {
    let session = state
        .store
        .get_session(session_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let provider = state
        .resolve_provider(session.provider_id)
        .await
        .map_err(|error| ApiError::status(StatusCode::CONFLICT, error.to_string()))?;
    let pipeline =
        DefaultTurnPipeline::with_lock(provider, Arc::clone(&state.store), state.turn_lock.clone());
    let response = pipeline
        .process_turn_debug(TurnRequestInput {
            session_id,
            input: request.input,
            mode: request.mode,
            viewer: ViewerContext::player(),
        })
        .await?;
    Ok(Json(DebugTurnResponseBody {
        message_id: response.turn.message_id,
        player_response: response.turn.player_response,
        scene_type: response.turn.scene_type,
        world_state_version: response.turn.world_state_version,
        changed_entities: response.turn.changed_entities,
        frontend_state_patch: response.turn.frontend_state_patch,
        applied_delta: response.applied_delta,
    }))
}

async fn turn_stream(
    State(state): State<AppState>,
    Path(session_id): Path<SessionId>,
    Json(request): Json<TurnRequest>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    // Resolve provider: session-scoped override takes priority over default.
    // Load session before entering the stream so we can pick the right provider.
    let (resolved_provider, provider_resolution_error) =
        match state.store.get_session(session_id).await {
            Ok(Some(session)) => match state.resolve_provider(session.provider_id).await {
                Ok(provider) => (provider, None),
                Err(error) => (Arc::clone(&state.provider), Some(error.to_string())),
            },
            Ok(None) => (Arc::clone(&state.provider), None),
            Err(_) => (Arc::clone(&state.provider), None),
        };

    let events = async_stream::stream! {
        if let Some(error) = provider_resolution_error {
            yield Ok(error_event(error));
            return;
        }

        let pipeline = Arc::new(DefaultTurnPipeline::with_lock(
            resolved_provider,
            Arc::clone(&state.store),
            state.turn_lock.clone(),
        ));

        let stream = engine::stream_turn(
            pipeline,
            engine::StreamTurnRequest {
                session_id,
                input: request.input,
                mode: request.mode,
                viewer: ViewerContext::player(),
            },
        );
        futures::pin_mut!(stream);

        let mut stream_usage = None;
        while let Some(event) = stream.next().await {
            match event {
                Ok(engine::StreamTurnEvent::Token(text)) => {
                    yield Ok(Event::default()
                        .event("token")
                        .json_data(TokenEvent { text })
                        .expect("token event serializes"));
                }
                Ok(engine::StreamTurnEvent::ProviderMetadata(meta)) => {
                    stream_usage = meta.usage.clone();
                }
                Ok(engine::StreamTurnEvent::Final(final_)) => {
                    let usage = stream_usage.clone().or(final_.provider_usage);
                    yield Ok(Event::default()
                        .event("final")
                        .json_data(StreamFinalEvent {
                            message_id: final_.message_id,
                            delta_applied: true,
                            world_state_version: final_.world_state_version,
                            frontend_state_patch: final_.frontend_state_patch,
                            usage,
                        })
                        .expect("final event serializes"));
                }
                Err(error) => {
                    yield Ok(error_event(error.to_string()));
                    return;
                }
            }
        }
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
struct RegisterProviderRequest {
    name: String,
    provider_type: String,
    base_url: String,
    model: String,
    api_key_secret_ref: Option<String>,
    capabilities: Option<serde_json::Value>,
    is_default: bool,
}

#[derive(Debug, Deserialize)]
struct SetProviderRequest {
    provider_id: Option<Uuid>,
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
struct DebugTurnResponseBody {
    message_id: Uuid,
    player_response: String,
    scene_type: domain::SceneReasoningStyle,
    world_state_version: i64,
    changed_entities: Vec<domain::EntityRef>,
    frontend_state_patch: domain::FrontendStatePatch,
    applied_delta: domain::WorldStateDelta,
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
    usage: Option<providers::TokenUsage>,
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
