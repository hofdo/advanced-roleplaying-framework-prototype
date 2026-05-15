use std::sync::Arc;

use anyhow::Result;
use clap::{Args as ClapArgs, ValueEnum};
use domain::{TurnMode, ViewerContext};
use engine::{DefaultTurnPipeline, TurnRequestInput, TurnResponse};
use uuid::Uuid;

use crate::{
    bootstrap::CliState,
    render::{print_json, render_streaming_turn},
};

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

    if args.stream {
        let provider = state.resolve_session_provider(args.session_id).await?;
        let pipeline = Arc::new(DefaultTurnPipeline::with_lock(
            provider,
            Arc::clone(&state.store),
            state.turn_lock.clone(),
        ));
        render_streaming_turn(pipeline, args.session_id, args.input, mode, viewer).await
    } else {
        let response =
            process_non_streaming_turn(&state, args.session_id, args.input, mode, viewer).await?;
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

async fn process_non_streaming_turn(
    state: &CliState,
    session_id: Uuid,
    input: String,
    mode: Option<TurnMode>,
    viewer: ViewerContext,
) -> Result<TurnResponse> {
    let provider = state.resolve_session_provider(session_id).await?;
    let pipeline = Arc::new(DefaultTurnPipeline::with_lock(
        provider,
        Arc::clone(&state.store),
        state.turn_lock.clone(),
    ));
    Ok(pipeline
        .process_turn(TurnRequestInput {
            session_id,
            input,
            mode,
            viewer,
        })
        .await?)
}

#[cfg(test)]
mod tests {
    use super::process_non_streaming_turn;
    use crate::bootstrap::CliState;
    use domain::{Location, Scenario, ScenarioType, TurnMode, ViewerContext};
    use engine::{InMemorySessionTurnLock, SessionTurnLock};
    use persistence::{ApplicationStore, InMemoryApplicationStore, ProviderRecord};
    use providers::{LlmProvider, build_provider_from_config};
    use serde_json::json;
    use shared::{AppConfig, ProviderConfig, StorageBackend};
    use std::sync::Arc;
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn provider_config(name: &str, base_url: &str) -> ProviderConfig {
        ProviderConfig {
            name: name.into(),
            provider_type: "openai_compatible".into(),
            base_url: base_url.into(),
            api_key: None,
            model: "test-model".into(),
            supports_streaming: true,
            supports_json_mode: true,
            max_context_tokens: None,
            request_timeout_seconds: 5,
            stream_idle_timeout_seconds: 5,
            max_retries: 0,
            http_referer: None,
            x_title: None,
            provider_routing: None,
            include_usage: true,
        }
    }

    fn provider_record(id: Uuid, name: &str, base_url: &str) -> ProviderRecord {
        ProviderRecord {
            id,
            name: name.into(),
            provider_type: "openai_compatible".into(),
            base_url: base_url.into(),
            model: "test-model".into(),
            api_key_secret_ref: None,
            capabilities: json!({
                "supports_streaming": true,
                "supports_json_mode": true,
                "supports_tool_calls": false,
                "supports_seed": false,
                "max_context_tokens": null,
                "request_timeout_seconds": 5,
                "stream_idle_timeout_seconds": 5,
                "max_retries": 0
            }),
            is_default: false,
        }
    }

    fn scenario() -> Scenario {
        Scenario {
            id: Uuid::new_v4(),
            title: "Override Turn".into(),
            scenario_type: ScenarioType::Adventure,
            setting: "test".into(),
            tone: "test".into(),
            rules: vec![],
            locations: vec![Location {
                id: "guildhall".into(),
                name: "Guildhall".into(),
                description: "The opening hall.".into(),
                visible: true,
            }],
            factions: vec![],
            npcs: vec![],
            quests: vec![],
            secrets: vec![],
            clocks: vec![],
        }
    }

    async fn mount_turn(server: &MockServer, visible: &str, event: &str) {
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": visible}}]
            })))
            .up_to_n_times(1)
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": json!({
                    "facts_to_add": [],
                    "npc_changes": [],
                    "faction_changes": [],
                    "quest_changes": [],
                    "clock_changes": [],
                    "relationship_changes": [],
                    "location_change": null,
                    "event_log_entries": [event]
                }).to_string()}}]
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn process_non_streaming_turn_uses_session_provider_override() {
        let default_server = MockServer::start().await;
        let override_server = MockServer::start().await;
        mount_turn(
            &default_server,
            "default provider used",
            "default provider event",
        )
        .await;
        mount_turn(
            &override_server,
            "session provider used",
            "session provider event",
        )
        .await;

        let store: Arc<dyn ApplicationStore> = Arc::new(InMemoryApplicationStore::new(false));
        let scenario = store
            .create_scenario(scenario())
            .await
            .expect("scenario");
        let session = store
            .create_session(scenario.id, "override-turn".into())
            .await
            .expect("create session")
            .expect("session");
        let provider_id = Uuid::new_v4();
        store
            .create_provider(provider_record(
                provider_id,
                "override-provider",
                &override_server.uri(),
            ))
            .await
            .expect("provider record");
        store
            .set_session_provider(session.id, Some(provider_id))
            .await
            .expect("set provider")
            .expect("session exists");

        let mut config = AppConfig::default();
        config.storage.backend = StorageBackend::Memory;
        config.provider.default = provider_config("default-provider", &default_server.uri());
        let provider: Arc<dyn LlmProvider> =
            build_provider_from_config(&config.provider.default).expect("default provider");
        let turn_lock: Arc<dyn SessionTurnLock> = Arc::new(InMemorySessionTurnLock::default());
        let state = CliState {
            config,
            store,
            provider,
            turn_lock,
        };

        let response = process_non_streaming_turn(
            &state,
            session.id,
            "I answer directly.".into(),
            Some(TurnMode::Dialogue),
            ViewerContext::player(),
        )
        .await
        .expect("turn response");

        assert_eq!(response.player_response, "session provider used");
    }
}
