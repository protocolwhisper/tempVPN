use std::{env, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use mpp::server::{axum::ChargeChallenger, tempo, Mpp, TempoConfig};
use tempvpn_coordinator_client::{ControlPlaneClient, ControlPlaneConfig};
use tempvpn_registry_aggregator::{parse_upstreams, router, AppState, FixedPaymentSettings};
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
    let mut state = AppState::new(upstreams, Duration::from_millis(timeout_ms))?;
    if env::var("REGISTRY_FIXED_PAYMENT_ENABLED").is_ok_and(|value| value == "true") {
        let realm = required("REGISTRY_MPP_REALM")?;
        let currency = required("REGISTRY_MPP_PAYMENT_CURRENCY")?;
        let recipient = required("REGISTRY_MPP_PAYMENT_RECIPIENT")?;
        let rpc_url = required("REGISTRY_MPP_RPC_URL")?;
        let chain_id = required("REGISTRY_MPP_CHAIN_ID")?.parse::<u64>()?;
        let coordinator = ControlPlaneClient::new(&ControlPlaneConfig {
            url: required("REGISTRY_COORDINATOR_URL")?,
            root_ca_path: PathBuf::from(required("REGISTRY_COORDINATOR_ROOT_CA_PATH")?),
            certificate_path: PathBuf::from(required("REGISTRY_COORDINATOR_CERTIFICATE_PATH")?),
            private_key_path: PathBuf::from(required("REGISTRY_COORDINATOR_PRIVATE_KEY_PATH")?),
        })
        .await?;
        let challenger = Mpp::create(
            tempo(TempoConfig {
                recipient: recipient.as_str(),
            })
            .currency(currency.as_str())
            .rpc_url(rpc_url.as_str())
            .chain_id(chain_id)
            .realm(realm.as_str()),
        )?;
        state = state.with_fixed_payments(
            Arc::new(coordinator),
            Arc::new(challenger) as Arc<dyn ChargeChallenger>,
            FixedPaymentSettings {
                realm,
                currency,
                recipient,
                max_duration_seconds: env::var("REGISTRY_MAX_DURATION_SECONDS")
                    .unwrap_or_else(|_| "3600".into())
                    .parse()?,
                grace_period_seconds: env::var("REGISTRY_GRACE_PERIOD_SECONDS")
                    .unwrap_or_else(|_| "604800".into())
                    .parse()?,
                node_control_token: required("REGISTRY_NODE_CONTROL_TOKEN")?,
            },
        );
    }
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "global registry aggregator listening");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

fn required(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    env::var(name)
        .map_err(|_| format!("{name} is required when registry fixed payment is enabled").into())
}
