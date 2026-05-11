use api::{AppState, PostgresApplicationStore, app_router};
use axum::{Router, body::Body, http::Request};
use engine::SessionTurnLock;
use http_body_util::BodyExt;
use persistence::PostgresSessionTurnLock;
use providers::LlmProvider;
use sqlx::{Executor, PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};
use tower::util::ServiceExt;

pub struct TestContext {
    pub router: Router,
    pub pool: PgPool,
    _container: Option<ContainerAsync<Postgres>>,
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
    let (database_url, container) = if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
        (url, None)
    } else {
        let container = Postgres::default()
            .with_db_name("roleplay")
            .with_user("roleplay")
            .with_password("roleplay")
            .start()
            .await?;
        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(5432).await?;
        (
            format!("postgres://roleplay:roleplay@{host}:{port}/roleplay"),
            Some(container),
        )
    };
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    reset_database(&pool).await?;

    let persistence = persistence::PgPersistence::from_pool(pool.clone());
    persistence.migrate().await?;

    let store = Arc::new(PostgresApplicationStore::new(
        persistence,
        config.debug.store_raw_provider_output,
    ));
    let turn_lock: Arc<dyn SessionTurnLock> = Arc::new(PostgresSessionTurnLock::new(pool.clone()));
    let state = AppState::from_parts(config, store, provider, turn_lock);

    Ok(TestContext {
        router: app_router(state),
        pool,
        _container: container,
    })
}

async fn reset_database(pool: &PgPool) -> anyhow::Result<()> {
    pool.execute("DROP SCHEMA IF EXISTS public CASCADE").await?;
    pool.execute("CREATE SCHEMA public").await?;
    Ok(())
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
