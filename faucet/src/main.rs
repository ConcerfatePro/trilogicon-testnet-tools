mod config;
mod db;
mod payout;
mod routes;

use crate::config::Config;
use crate::payout::default_payout_adapter;
use crate::routes::{app_router, AppState};
use anyhow::Context;
use axum::serve;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let config = Config::from_env().map_err(anyhow::Error::msg)?;
    let pool = db::connect(&config.database_url)
        .await
        .with_context(|| format!("open database {}", config.database_url))?;

    let bind = &config.bind_addr;
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("bind to {bind}"))?;

    tracing::info!(%bind, dry_run = config.dry_run, "starting trilogicon-faucet");

    let state = AppState {
        config: Arc::new(config),
        pool,
        payout: default_payout_adapter(),
    };

    let app = app_router(state);
    serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("server error")?;

    Ok(())
}
