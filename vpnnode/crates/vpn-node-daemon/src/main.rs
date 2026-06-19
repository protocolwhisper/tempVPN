mod cleanup;
mod config;
mod error;
mod ip_allocator;
mod routes;
mod sessions;
mod wireguard;

use clap::Parser;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    cleanup::spawn_expiry_loop,
    config::{Args, Config},
    error::Result,
    routes::{router, AppState},
    sessions::Sessions,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vpn_node_daemon=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::load(Args::parse()).await?;
    let sessions = Sessions::new(&config)?;
    spawn_expiry_loop(sessions.clone(), config.sweep_interval_seconds);

    let listener = TcpListener::bind(config.bind_addr).await?;
    let app = router(AppState {
        config: config.clone(),
        sessions: sessions.clone(),
    });

    info!(addr = %config.bind_addr, "vpn-node-daemon listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    if config.cleanup_on_shutdown {
        info!("cleaning up active sessions before shutdown");
        sessions.cleanup_all().await;
    }

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
