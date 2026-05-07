use anyhow::Context;
use api::{AppState, app_router};
use shared::AppConfig;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "api=info,engine=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = AppConfig::load(None).context("failed to load configuration")?;
    let addr = SocketAddr::from((config.server.host, config.server.port));
    let state = AppState::new(config)
        .await
        .context("failed to initialize application state")?;
    let app = app_router(state).layer(TraceLayer::new_for_http());
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind HTTP listener on {addr}"))?;

    tracing::info!(%addr, "roleplaying engine API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server failed")?;

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
