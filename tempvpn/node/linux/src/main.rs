mod cleanup;
mod config;
mod error;
mod helpers;
mod ip_allocator;
mod location;
mod reconciliation;
mod registry;
mod routes;
mod session_v2;
mod sessions;
mod wireguard;

use alloy::{network::EthereumWallet, providers::ProviderBuilder};
use clap::Parser;
use mpp::server::{axum::ChargeChallenger, tempo, Mpp, TempoConfig};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    cleanup::spawn_expiry_loop,
    config::{Args, Config},
    error::{Error, Result},
    routes::{router, AppState},
    session_v2::{
        chain::TempoReserveChain,
        method::{SessionV2Config, TempoSessionV2Method},
        store::SessionStore,
        StreamingPayments,
    },
    sessions::Sessions,
    wireguard::WireGuard,
};
use tempo_alloy::TempoNetwork;
use tempvpn_coordinator_client::{
    ClientConfig as CoordinatorClientConfig, CoordinatorClient, GenerationMetadata,
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
    let coordinator = create_coordinator_client(&config).await?;
    let reconciler = coordinator.as_ref().map(|coordinator| {
        reconciliation::PeerReconciler::new(
            coordinator.clone(),
            WireGuard::new(
                config.wg_command.clone(),
                config.wg_interface.clone(),
                config.mock_wg,
            ),
        )
    });
    let coordinated_peer_count = reconciler
        .as_ref()
        .map(|reconciler| reconciler.managed_count_handle());
    if let Some(coordinator) = &coordinator {
        coordinator
            .register_generation(
                &GenerationMetadata {
                    node_name: config.node_name.clone(),
                    region: config.node_region.clone(),
                    country_code: config.node_country_code.clone(),
                    subdivision_code: config.node_subdivision_code.clone(),
                    city: config.node_city.clone(),
                    api_url: config.public_api_url.clone(),
                    wireguard_endpoint: config.endpoint.clone(),
                    wireguard_public_key: config.server_public_key.clone(),
                    expected_exit_ip: config.expected_exit_ip.clone(),
                    tunnel_network: config.tunnel_cidr.clone(),
                },
                available_slots(&sessions, coordinated_peer_count.as_deref()).await as u32,
            )
            .await?;
        spawn_coordinator_renewal(
            coordinator.clone(),
            sessions.clone(),
            coordinated_peer_count.clone(),
        );
    }
    if let Some(reconciler) = &reconciler {
        reconciler.spawn();
    }
    let challenger = create_mpp_challenger(&config)?;
    let registry = registry::Registry::default();
    let streaming = create_streaming_payments(&config, sessions.clone()).await?;
    if let Some(streaming) = streaming.clone() {
        spawn_lease_reaper(
            streaming,
            sessions.clone(),
            config.streaming.grace_period_seconds,
        );
    }
    spawn_expiry_loop(sessions.clone(), config.sweep_interval_seconds);
    let registration = registry::spawn_registration(
        config.clone(),
        sessions.clone(),
        coordinated_peer_count.clone(),
    );

    let listener = TcpListener::bind(config.bind_addr).await?;
    let app = router(AppState {
        config: config.clone(),
        sessions: sessions.clone(),
        challenger,
        registry,
        coordinator,
        coordinated_peer_count,
        streaming,
    });

    info!(addr = %config.bind_addr, "vpn-node-daemon listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    if let Some(registration) = registration {
        registration.abort();
    }
    registry::unregister(&config).await;

    if config.cleanup_on_shutdown {
        info!("cleaning up active sessions before shutdown");
        if let Some(reconciler) = reconciler {
            reconciler.cleanup().await;
        }
        sessions.cleanup_all().await;
    }

    Ok(())
}

async fn create_coordinator_client(config: &Config) -> Result<Option<Arc<CoordinatorClient>>> {
    let Some(coordinator) = &config.coordinator else {
        return Ok(None);
    };
    Ok(Some(Arc::new(
        CoordinatorClient::new(&CoordinatorClientConfig {
            url: coordinator.url.clone(),
            logical_node: coordinator.logical_node.clone(),
            generation_id: coordinator.generation_id.clone(),
            root_ca_path: coordinator.root_ca_path.clone(),
            certificate_path: coordinator.certificate_path.clone(),
            private_key_path: coordinator.private_key_path.clone(),
        })
        .await?,
    )))
}

fn spawn_coordinator_renewal(
    client: Arc<CoordinatorClient>,
    sessions: Arc<Sessions>,
    coordinated_peer_count: Option<Arc<AtomicUsize>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            if let Err(error) = client
                .renew_generation(
                    available_slots(&sessions, coordinated_peer_count.as_deref()).await as u32,
                )
                .await
            {
                error!(error = %error, "failed to renew coordinator generation health");
            }
        }
    });
}

async fn available_slots(sessions: &Sessions, coordinated: Option<&AtomicUsize>) -> usize {
    sessions.available_slots().await.saturating_sub(
        coordinated
            .map(|count| count.load(Ordering::Relaxed))
            .unwrap_or(0),
    )
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn create_mpp_challenger(config: &Config) -> Result<Arc<dyn ChargeChallenger>> {
    let mpp = Mpp::create(
        tempo(TempoConfig {
            recipient: config.mpp_payment_recipient.as_str(),
        })
        .currency(config.mpp_payment_currency.as_str())
        .rpc_url(config.mpp_rpc_url.as_str())
        .realm(config.mpp_realm.as_str()),
    )
    .map_err(|err| Error::Mpp(err.to_string()))?;

    Ok(Arc::new(mpp) as Arc<dyn ChargeChallenger>)
}

async fn create_streaming_payments(
    config: &Config,
    sessions: Arc<Sessions>,
) -> Result<Option<Arc<StreamingPayments>>> {
    if !config.streaming.enabled {
        return Ok(None);
    }

    let signer =
        config.streaming.close_signer.clone().ok_or_else(|| {
            Error::InvalidConfig("streaming close signer was not validated".into())
        })?;
    let payee = config
        .mpp_payment_recipient
        .parse()
        .map_err(|_| Error::InvalidConfig("MPP recipient is not an address".into()))?;
    let token = config
        .mpp_payment_currency
        .parse()
        .map_err(|_| Error::InvalidConfig("MPP currency is not an address".into()))?;
    if signer.address() != payee
        && (config.streaming.operator == alloy::primitives::Address::ZERO
            || signer.address() != config.streaming.operator)
    {
        return Err(Error::InvalidConfig(
            "streaming close signer must match the configured recipient or non-zero operator"
                .into(),
        ));
    }
    let rpc_url = config
        .mpp_rpc_url
        .parse()
        .map_err(|error| Error::InvalidConfig(format!("invalid Tempo RPC URL: {error}")))?;
    let provider = ProviderBuilder::new_with_network::<TempoNetwork>()
        .wallet(EthereumWallet::from(signer.clone()))
        .connect_http(rpc_url);
    let chain = Arc::new(TempoReserveChain::new(
        provider,
        config.streaming.reserve,
        config.streaming.chain_id,
        signer.address(),
    ));
    let store = SessionStore::open(&config.streaming.store).await?;
    if config.streaming.mode == config::StreamingMode::Production && !store.is_durable() {
        return Err(Error::InvalidConfig(
            "production Session v2 store did not initialize as durable".into(),
        ));
    }
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64;
    for (channel_id, lease) in store
        .reconcile_startup_leases(now_unix)
        .await
        .map_err(|error| Error::Store(error.to_string()))?
    {
        let cleanup_owner = format!("reaper_{}", uuid::Uuid::new_v4().simple());
        let claimed = store
            .claim_expired_lease(
                &channel_id,
                &lease.owner_id,
                &cleanup_owner,
                now_unix,
                now_unix.saturating_add(60),
            )
            .await
            .map_err(|error| Error::Store(error.to_string()))?;
        if !claimed {
            continue;
        }
        sessions
            .disable_peer_by_public_key(&lease.client_public_key)
            .await?;
        store
            .release_lease(&channel_id, &cleanup_owner)
            .await
            .map_err(|error| Error::Store(error.to_string()))?;
    }
    let method = TempoSessionV2Method::new(
        chain,
        store.clone(),
        SessionV2Config {
            reserve: config.streaming.reserve,
            chain_id: config.streaming.chain_id,
            operator: config.streaming.operator,
            payee,
            token,
            unit_amount: config.streaming.unit_amount,
            min_voucher_delta: config.streaming.min_voucher_delta,
        },
    );
    let mpp = Mpp::create(
        tempo(TempoConfig {
            recipient: config.mpp_payment_recipient.as_str(),
        })
        .currency(config.mpp_payment_currency.as_str())
        .rpc_url(config.mpp_rpc_url.as_str())
        .chain_id(config.streaming.chain_id)
        .realm(config.mpp_realm.as_str()),
    )
    .map_err(|error| Error::Mpp(error.to_string()))?
    .with_session_method(method);

    Ok(Some(Arc::new(StreamingPayments { mpp, store })))
}

fn spawn_lease_reaper(
    streaming: Arc<StreamingPayments>,
    sessions: Arc<Sessions>,
    interval_seconds: u64,
) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(interval_seconds.max(1)));
        loop {
            interval.tick().await;
            let now_unix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .min(i64::MAX as u64) as i64;
            let leases = match streaming.store.reconcile_startup_leases(now_unix).await {
                Ok(leases) => leases,
                Err(error) => {
                    error!(error = %error, "failed to scan expired Session v2 leases");
                    continue;
                }
            };
            for (channel_id, lease) in leases {
                let cleanup_owner = format!("reaper_{}", uuid::Uuid::new_v4().simple());
                let claimed = match streaming
                    .store
                    .claim_expired_lease(
                        &channel_id,
                        &lease.owner_id,
                        &cleanup_owner,
                        now_unix,
                        now_unix.saturating_add(60),
                    )
                    .await
                {
                    Ok(claimed) => claimed,
                    Err(error) => {
                        error!(channel_id, error = %error, "failed to claim expired Session v2 lease");
                        continue;
                    }
                };
                if !claimed {
                    continue;
                }
                if let Err(error) = sessions
                    .disable_peer_by_public_key(&lease.client_public_key)
                    .await
                {
                    error!(
                        channel_id,
                        error = %error,
                        "failed to remove peer for expired Session v2 lease"
                    );
                    continue;
                }
                if let Err(error) = streaming
                    .store
                    .release_lease(&channel_id, &cleanup_owner)
                    .await
                {
                    error!(channel_id, error = %error, "failed to release expired Session v2 lease");
                }
            }
        }
    });
}
