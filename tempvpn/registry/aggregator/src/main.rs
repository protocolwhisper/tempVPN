use std::{env, net::SocketAddr, time::Duration};

use tempvpn_registry_aggregator::{parse_upstreams, router, AppState};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let upstreams = parse_upstreams(&env::var("REGISTRY_UPSTREAMS")?)?;
    let timeout_ms = env::var("UPSTREAM_TIMEOUT_MS")
        .unwrap_or_else(|_| "3000".into())
        .parse::<u64>()?;
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()?;
    let state = AppState::new(upstreams, Duration::from_millis(timeout_ms))?;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "global registry aggregator listening");
    axum::serve(listener, router(state)).await?;
    Ok(())
}
