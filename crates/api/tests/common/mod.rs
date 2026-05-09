use api::{app_router, AppState, ApiStore, PostgresApplicationStore};
use axum::{body::Body, http::Request, Router};
use domain::{
    Faction, FactionIdentity, Location, Npc, NpcStatus, Quest, RoleIdentity, Scenario,
    ScenarioType, Secret,
};
use engine::{InMemorySessionTurnLock, SessionTurnLock};
use http_body_util::BodyExt;
use persistence::PostgresSessionTurnLock;
use providers::{LlmProvider, MockProvider};
use serde::de::DeserializeOwned;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::sync::Arc;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};
use tower::util::ServiceExt;
use uuid::Uuid;

pub struct TestContext {
    pub router: Router,
    pub pool: PgPool,
    _container: ContainerAsync<Postgres>,
}

impl TestContext {
    pub async fn cleanup(self) {
        self.pool.close().await;
    }
}

pub async fn postgres_test_context(
    provider: Arc<dyn LlmProvider>,
) -> anyhow::Result<TestContext> {
    postgres_test_context_with_config(provider, {
        let mut config = shared::AppConfig::default();
        config.storage.backend = shared::StorageBackend::Postgres;
        config
    })
    .await
}

pub async fn postgres_test_context_with_config(
    provider: Arc<dyn LlmProvider>,
    config: shared::AppConfig,
) -> anyhow::Result<TestContext> {
    let container = Postgres::default()
        .with_db_name("roleplay")
        .with_user("roleplay")
        .with_password("roleplay")
        .start()
        .await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let database_url = format!("postgres://roleplay:roleplay@{host}:{port}/roleplay");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    let persistence = persistence::PgPersistence::from_pool(pool.clone());
    persistence.migrate().await?;

    let store = Arc::new(PostgresApplicationStore::new(
        persistence,
        config.debug.store_raw_provider_output,
    ));
    let turn_lock: Arc<dyn SessionTurnLock> =
        Arc::new(PostgresSessionTurnLock::new(pool.clone()));
    let state = AppState::from_parts(config, store, provider, turn_lock);

    Ok(TestContext {
        router: app_router(state),
        pool,
        _container: container,
    })
}

pub fn mock_provider(responses: impl IntoIterator<Item = String>) -> Arc<dyn LlmProvider> {
    Arc::new(MockProvider::new("mock", responses))
}

pub async fn send_json(
    router: &Router,
    method: &str,
    path: &str,
    value: serde_json::Value,
) -> (http::StatusCode, bytes::Bytes) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collection")
        .to_bytes();
    (status, body)
}

pub async fn send_empty(
    router: &Router,
    method: &str,
    path: &str,
) -> (http::StatusCode, bytes::Bytes) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collection")
        .to_bytes();
    (status, body)
}

pub async fn send_json_with_bearer(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    value: serde_json::Value,
) -> (http::StatusCode, bytes::Bytes) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(value.to_string()))
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collection")
        .to_bytes();
    (status, body)
}

pub async fn send_empty_with_bearer(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
) -> (http::StatusCode, bytes::Bytes) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collection")
        .to_bytes();
    (status, body)
}

pub fn json_body<T: DeserializeOwned>(body: &[u8]) -> T {
    serde_json::from_slice(body).expect("json body")
}

pub fn memory_test_context(provider: Arc<dyn LlmProvider>) -> Router {
    memory_test_context_with_config(provider, {
        let mut config = shared::AppConfig::default();
        config.storage.backend = shared::StorageBackend::Memory;
        config
    })
}

pub fn memory_test_context_with_config(
    provider: Arc<dyn LlmProvider>,
    config: shared::AppConfig,
) -> Router {
    let store_raw_provider_output = config.debug.store_raw_provider_output;
    let state = AppState::from_parts(
        config,
        Arc::new(ApiStore::new(store_raw_provider_output)),
        provider,
        Arc::new(InMemorySessionTurnLock::default()),
    );
    app_router(state)
}

pub fn sample_scenario() -> Scenario {
    Scenario {
        id: Uuid::new_v4(),
        title: "Chosen Beyond the Goddess".into(),
        scenario_type: ScenarioType::Adventure,
        setting: "A high fantasy isekai world of sword and magic.".into(),
        tone: "heroic, consequence-driven, high fantasy".into(),
        rules: vec![],
        locations: vec![Location {
            id: "guildhall".into(),
            name: "Guildhall".into(),
            description: "A busy hall filled with examiners and witnesses.".into(),
            visible: true,
        }],
        factions: vec![Faction {
            id: "guild".into(),
            name: "Continental Adventurer Guild".into(),
            description: "Ranks adventurers and monitors dangerous anomalies.".into(),
            faction_identity: FactionIdentity {
                public_goal: "assign quests and protect settlements".into(),
                hidden_goal: Some("monitor calamity-level individuals".into()),
                values: vec!["competence".into(), "contracts".into()],
                fears: vec!["public panic".into()],
                methods: vec!["ranking exams".into()],
            },
            initial_standing: 0,
        }],
        npcs: vec![Npc {
            id: "examiner".into(),
            name: "Guild Examiner".into(),
            description: "A veteran examiner with a careful eye.".into(),
            role_identity: RoleIdentity {
                core_emotion: "alert".into(),
                motivation: "protect civilians while evaluating the player".into(),
                worldview: "power demands accountability".into(),
                fear: Some("uncontrolled magical catastrophe".into()),
                desire: None,
                speech_style: "measured and formal".into(),
                boundaries: vec!["will not ignore public danger".into()],
                values: vec!["order".into()],
            },
            stats: None,
            initial_status: NpcStatus::Active,
        }],
        quests: vec![Quest {
            id: "register".into(),
            title: "Register at the Guild".into(),
            description: "Complete the registration process.".into(),
            objectives: vec![],
            visible: true,
        }],
        secrets: vec![Secret {
            id: "void-mark".into(),
            text: "The player's soul-mark was not created by the goddess.".into(),
            reveal_conditions: vec!["a divine relic reacts to the mark".into()],
        }],
        clocks: vec![domain::ClockTemplate {
            id: "fame".into(),
            title: "The player's fame spreads".into(),
            current: 1,
            max: 6,
            consequence: "Major factions start treating the player as a strategic threat.".into(),
        }],
    }
}
