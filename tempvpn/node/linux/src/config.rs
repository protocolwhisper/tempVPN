use std::{env, net::SocketAddr, path::PathBuf, str::FromStr};

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use clap::Parser;
use mpp::protocol::methods::tempo::PRECOMPILE_MAX_CUMULATIVE_AMOUNT;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::helpers::endpoint_host;
use crate::location::{normalize_country_code, normalize_optional_text};

const DEFAULT_MPP_RPC_URL: &str = "https://rpc.moderato.tempo.xyz";
const DEFAULT_MPP_REALM: &str = "localhost:8080";
const DEFAULT_MPP_PAYMENT_CURRENCY: &str = "0x20c0000000000000000000000000000000000000";
const DEFAULT_MPP_PAYMENT_RECIPIENT: &str = "0xB01E80a8CD7C72589f30D2004aeb60937a2150d3";
const DEFAULT_MPP_CHAIN_ID: u64 = 42_431;
const DEFAULT_MPP_SESSION_RESERVE: &str = "0x4d50500000000000000000000000000000000000";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingMode {
    Development,
    Production,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelStoreConfig {
    Memory,
    Sqlite(PathBuf),
}

#[derive(Clone)]
pub struct StreamingConfig {
    pub enabled: bool,
    pub mode: StreamingMode,
    pub chain_id: u64,
    pub reserve: Address,
    pub operator: Address,
    pub unit_amount: u128,
    pub billing_interval_seconds: u64,
    pub suggested_reserve: u128,
    pub min_voucher_delta: u128,
    pub grace_period_seconds: u64,
    pub close_signer: Option<PrivateKeySigner>,
    pub store: ChannelStoreConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordinatorConfig {
    pub url: String,
    pub logical_node: String,
    pub generation_id: String,
    pub root_ca_path: PathBuf,
    pub certificate_path: PathBuf,
    pub private_key_path: PathBuf,
}

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub admin_token: String,
    pub node_id: String,
    pub node_name: String,
    pub node_region: String,
    pub node_country_code: Option<String>,
    pub node_subdivision_code: Option<String>,
    pub node_city: Option<String>,
    pub accepting_sessions: bool,
    pub public_api_url: String,
    pub expected_exit_ip: String,
    pub registry_mode: bool,
    pub registry_url: Option<String>,
    pub registry_token: Option<String>,
    pub registry_refresh_seconds: u64,
    pub registry_lease_seconds: u64,
    pub wg_interface: String,
    pub wg_command: String,
    pub server_public_key: String,
    pub endpoint: String,
    pub tunnel_cidr: String,
    pub max_duration_seconds: u64,
    pub grace_period_seconds: u64,
    pub stale_timeout_seconds: u64,
    pub sweep_interval_seconds: u64,
    pub cleanup_on_shutdown: bool,
    pub mock_wg: bool,
    pub mpp_rpc_url: String,
    pub mpp_realm: String,
    pub mpp_payment_currency: String,
    pub mpp_payment_recipient: String,
    pub coordinator: Option<CoordinatorConfig>,
    pub streaming: StreamingConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct FileConfig {
    bind_addr: Option<SocketAddr>,
    admin_token: Option<String>,
    node_id: Option<String>,
    node_name: Option<String>,
    node_region: Option<String>,
    node_country_code: Option<String>,
    node_subdivision_code: Option<String>,
    node_city: Option<String>,
    accepting_sessions: Option<bool>,
    public_api_url: Option<String>,
    expected_exit_ip: Option<String>,
    registry_mode: Option<bool>,
    registry_url: Option<String>,
    registry_token: Option<String>,
    registry_refresh_seconds: Option<u64>,
    registry_lease_seconds: Option<u64>,
    wg_interface: Option<String>,
    wg_command: Option<String>,
    server_public_key: Option<String>,
    endpoint: Option<String>,
    tunnel_cidr: Option<String>,
    max_duration_seconds: Option<u64>,
    grace_period_seconds: Option<u64>,
    stale_timeout_seconds: Option<u64>,
    sweep_interval_seconds: Option<u64>,
    cleanup_on_shutdown: Option<bool>,
    mock_wg: Option<bool>,
    mpp_rpc_url: Option<String>,
    mpp_realm: Option<String>,
    mpp_payment_currency: Option<String>,
    mpp_payment_recipient: Option<String>,
    fixed_session_mode: Option<String>,
    coordinator_url: Option<String>,
    coordinator_logical_node: Option<String>,
    coordinator_generation_id: Option<String>,
    coordinator_root_ca_path: Option<PathBuf>,
    coordinator_certificate_path: Option<PathBuf>,
    coordinator_private_key_path: Option<PathBuf>,
    mpp_streaming_enabled: Option<bool>,
    mpp_streaming_mode: Option<String>,
    mpp_chain_id: Option<u64>,
    mpp_session_reserve: Option<String>,
    mpp_session_operator: Option<String>,
    mpp_session_unit_amount: Option<u128>,
    mpp_session_billing_interval_seconds: Option<u64>,
    mpp_session_suggested_reserve: Option<u128>,
    mpp_session_min_voucher_delta: Option<u128>,
    mpp_session_grace_period_seconds: Option<u64>,
    mpp_session_close_private_key: Option<String>,
    mpp_session_store: Option<String>,
    mpp_session_sqlite_path: Option<PathBuf>,
}

impl Config {
    pub async fn load(args: Args) -> Result<Self> {
        let file = match args.config {
            Some(path) => {
                let contents =
                    tokio::fs::read_to_string(&path)
                        .await
                        .map_err(|source| Error::ConfigRead {
                            path: path.clone(),
                            source,
                        })?;
                toml::from_str::<FileConfig>(&contents)
                    .map_err(|source| Error::ConfigParse { path, source })?
            }
            None => FileConfig::default(),
        };

        let streaming_file = file.clone();
        let bind_addr = env_or("VPN_NODE_BIND_ADDR", file.bind_addr, "0.0.0.0:8080")?;
        let admin_token = env_or_required("VPN_NODE_ADMIN_TOKEN", file.admin_token)?;
        let node_id = env_or_default("VPN_NODE_ID", file.node_id, "default");
        let node_name = env_or_default("VPN_NODE_NAME", file.node_name, "Tempo VPN Node");
        let node_region = env_or_default("VPN_NODE_REGION", file.node_region, "unknown");
        let node_country_code = normalize_country_code(
            env_or_optional("VPN_NODE_COUNTRY_CODE", file.node_country_code).as_deref(),
        )
        .map_err(Error::InvalidConfig)?;
        let node_subdivision_code = normalize_optional_text(
            "node_subdivision_code",
            env_or_optional("VPN_NODE_SUBDIVISION_CODE", file.node_subdivision_code).as_deref(),
        )
        .map_err(Error::InvalidConfig)?;
        let node_city = normalize_optional_text(
            "node_city",
            env_or_optional("VPN_NODE_CITY", file.node_city).as_deref(),
        )
        .map_err(Error::InvalidConfig)?;
        let accepting_sessions = env_or(
            "VPN_NODE_ACCEPTING_SESSIONS",
            file.accepting_sessions,
            "true",
        )?;
        let public_api_url = env_or_default(
            "VPN_NODE_PUBLIC_API_URL",
            file.public_api_url,
            &format!("http://{bind_addr}"),
        );
        let registry_mode = env_or("VPN_NODE_REGISTRY_MODE", file.registry_mode, "false")?;
        let registry_url = env_var("VPN_NODE_REGISTRY_URL")
            .or_else(|| file.registry_url.filter(|value| !value.is_empty()));
        let registry_token = env_var("VPN_NODE_REGISTRY_TOKEN")
            .or_else(|| file.registry_token.filter(|value| !value.is_empty()));
        let registry_refresh_seconds = env_or(
            "VPN_NODE_REGISTRY_REFRESH_SECONDS",
            file.registry_refresh_seconds,
            "30",
        )?;
        let registry_lease_seconds = env_or(
            "VPN_NODE_REGISTRY_LEASE_SECONDS",
            file.registry_lease_seconds,
            "90",
        )?;
        if registry_mode && registry_token.is_none() {
            return Err(Error::InvalidConfig(
                "registry_token is required when registry_mode is enabled".into(),
            ));
        }
        if registry_url.is_some() && registry_token.is_none() {
            return Err(Error::InvalidConfig(
                "registry_token is required when registry_url is configured".into(),
            ));
        }
        if registry_token.as_deref() == Some(admin_token.as_str()) {
            return Err(Error::InvalidConfig(
                "registry_token must be different from admin_token".into(),
            ));
        }
        let wg_interface = env_or_default("VPN_NODE_WG_INTERFACE", file.wg_interface, "wg0");
        let wg_command = env_or_default("VPN_NODE_WG_COMMAND", file.wg_command, "wg");
        let server_public_key =
            env_or_required("VPN_NODE_SERVER_PUBLIC_KEY", file.server_public_key)?;
        let endpoint = env_or_required("VPN_NODE_ENDPOINT", file.endpoint)?;
        let expected_exit_ip = env_or_default(
            "VPN_NODE_EXPECTED_EXIT_IP",
            file.expected_exit_ip,
            endpoint_host(&endpoint),
        );
        let tunnel_cidr = env_or_default("VPN_NODE_TUNNEL_CIDR", file.tunnel_cidr, "10.8.0.0/24");
        let max_duration_seconds = env_or(
            "VPN_NODE_MAX_DURATION_SECONDS",
            file.max_duration_seconds,
            "3600",
        )?;
        let grace_period_seconds = env_or(
            "VPN_NODE_GRACE_PERIOD_SECONDS",
            file.grace_period_seconds,
            "604800",
        )?;
        let stale_timeout_seconds = env_or(
            "VPN_NODE_STALE_TIMEOUT_SECONDS",
            file.stale_timeout_seconds,
            "90",
        )?;
        let sweep_interval_seconds = env_or(
            "VPN_NODE_SWEEP_INTERVAL_SECONDS",
            file.sweep_interval_seconds,
            "10",
        )?;
        let cleanup_on_shutdown = env_or(
            "VPN_NODE_CLEANUP_ON_SHUTDOWN",
            file.cleanup_on_shutdown,
            "true",
        )?;
        let mock_wg = env_or("VPN_NODE_MOCK_WG", file.mock_wg, "false")?;
        let mpp_rpc_url = env_or_default(
            "VPN_NODE_MPP_RPC_URL",
            file.mpp_rpc_url,
            DEFAULT_MPP_RPC_URL,
        );
        let mpp_realm = env_or_default("VPN_NODE_MPP_REALM", file.mpp_realm, DEFAULT_MPP_REALM);
        let mpp_payment_currency = env_or_default(
            "VPN_NODE_MPP_PAYMENT_CURRENCY",
            file.mpp_payment_currency,
            DEFAULT_MPP_PAYMENT_CURRENCY,
        );
        let mpp_payment_recipient = env_or_default(
            "VPN_NODE_MPP_PAYMENT_RECIPIENT",
            file.mpp_payment_recipient,
            DEFAULT_MPP_PAYMENT_RECIPIENT,
        );
        let fixed_session_mode = env_or_default(
            "VPN_NODE_FIXED_SESSION_MODE",
            file.fixed_session_mode.clone(),
            "memory",
        );
        let coordinator = match fixed_session_mode.to_ascii_lowercase().as_str() {
            "memory" => None,
            "coordinator" => Some(CoordinatorConfig {
                url: env_or_required("VPN_NODE_COORDINATOR_URL", file.coordinator_url)?
                    .trim_end_matches('/')
                    .to_string(),
                logical_node: env_or_default(
                    "VPN_NODE_COORDINATOR_LOGICAL_NODE",
                    file.coordinator_logical_node,
                    &node_id,
                ),
                generation_id: env_or_required(
                    "VPN_NODE_COORDINATOR_GENERATION_ID",
                    file.coordinator_generation_id,
                )?,
                root_ca_path: env_path_required(
                    "VPN_NODE_COORDINATOR_ROOT_CA_FILE",
                    file.coordinator_root_ca_path,
                )?,
                certificate_path: env_path_required(
                    "VPN_NODE_COORDINATOR_CERT_FILE",
                    file.coordinator_certificate_path,
                )?,
                private_key_path: env_path_required(
                    "VPN_NODE_COORDINATOR_KEY_FILE",
                    file.coordinator_private_key_path,
                )?,
            }),
            other => {
                return Err(Error::InvalidConfig(format!(
                    "VPN_NODE_FIXED_SESSION_MODE must be memory or coordinator, got {other}"
                )))
            }
        };
        let streaming = load_streaming_config(&streaming_file, &mpp_payment_recipient)?;
        validate_production_payment_identity(
            &streaming,
            &mpp_rpc_url,
            &mpp_realm,
            &mpp_payment_currency,
            &mpp_payment_recipient,
        )?;

        Ok(Self {
            bind_addr,
            admin_token,
            node_id,
            node_name,
            node_region,
            node_country_code,
            node_subdivision_code,
            node_city,
            accepting_sessions,
            public_api_url,
            expected_exit_ip,
            registry_mode,
            registry_url,
            registry_token,
            registry_refresh_seconds,
            registry_lease_seconds,
            wg_interface,
            wg_command,
            server_public_key,
            endpoint,
            tunnel_cidr,
            max_duration_seconds,
            grace_period_seconds,
            stale_timeout_seconds,
            sweep_interval_seconds,
            cleanup_on_shutdown,
            mock_wg,
            mpp_rpc_url,
            mpp_realm,
            mpp_payment_currency,
            mpp_payment_recipient,
            coordinator,
            streaming,
        })
    }
}

fn load_streaming_config(file: &FileConfig, recipient: &str) -> Result<StreamingConfig> {
    let enabled = env_or(
        "VPN_NODE_MPP_STREAMING_ENABLED",
        file.mpp_streaming_enabled,
        "false",
    )?;
    let mode = match env_or_default(
        "VPN_NODE_MPP_STREAMING_MODE",
        file.mpp_streaming_mode.clone(),
        "development",
    )
    .to_ascii_lowercase()
    .as_str()
    {
        "development" => StreamingMode::Development,
        "production" => StreamingMode::Production,
        other => {
            return Err(Error::InvalidConfig(format!(
                "VPN_NODE_MPP_STREAMING_MODE must be development or production, got {other}"
            )))
        }
    };

    let chain_id = env_or(
        "VPN_NODE_MPP_CHAIN_ID",
        file.mpp_chain_id,
        &DEFAULT_MPP_CHAIN_ID.to_string(),
    )?;
    let reserve_raw = env_or_default(
        "VPN_NODE_MPP_SESSION_RESERVE",
        file.mpp_session_reserve.clone(),
        DEFAULT_MPP_SESSION_RESERVE,
    );
    let operator_raw = env_or_default(
        "VPN_NODE_MPP_SESSION_OPERATOR",
        file.mpp_session_operator.clone(),
        recipient,
    );
    let unit_amount = env_or(
        "VPN_NODE_MPP_SESSION_UNIT_AMOUNT",
        file.mpp_session_unit_amount,
        "1000",
    )?;
    let billing_interval_seconds = env_or(
        "VPN_NODE_MPP_SESSION_BILLING_INTERVAL_SECONDS",
        file.mpp_session_billing_interval_seconds,
        "60",
    )?;
    let suggested_reserve = env_or(
        "VPN_NODE_MPP_SESSION_SUGGESTED_RESERVE",
        file.mpp_session_suggested_reserve,
        "10000",
    )?;
    let min_voucher_delta = env_or(
        "VPN_NODE_MPP_SESSION_MIN_VOUCHER_DELTA",
        file.mpp_session_min_voucher_delta,
        "1000",
    )?;
    let grace_period_seconds = env_or(
        "VPN_NODE_MPP_SESSION_GRACE_PERIOD_SECONDS",
        file.mpp_session_grace_period_seconds,
        "30",
    )?;
    let close_key = env::var("VPN_NODE_MPP_SESSION_CLOSE_PRIVATE_KEY")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| file.mpp_session_close_private_key.clone());
    let store_kind = env_or_default(
        "VPN_NODE_MPP_SESSION_STORE",
        file.mpp_session_store.clone(),
        "memory",
    );
    let sqlite_path = env::var_os("VPN_NODE_MPP_SESSION_SQLITE_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| file.mpp_session_sqlite_path.clone());

    let reserve = parse_address("VPN_NODE_MPP_SESSION_RESERVE", &reserve_raw)?;
    let operator = parse_address("VPN_NODE_MPP_SESSION_OPERATOR", &operator_raw)?;
    let close_signer = close_key
        .as_deref()
        .map(|key| {
            PrivateKeySigner::from_str(key).map_err(|_| {
                Error::InvalidConfig(
                    "VPN_NODE_MPP_SESSION_CLOSE_PRIVATE_KEY is not a valid private key".into(),
                )
            })
        })
        .transpose()?;
    let store = match store_kind.to_ascii_lowercase().as_str() {
        "memory" => ChannelStoreConfig::Memory,
        "sqlite" => ChannelStoreConfig::Sqlite(sqlite_path.ok_or_else(|| {
            Error::InvalidConfig(
                "VPN_NODE_MPP_SESSION_SQLITE_PATH is required for the sqlite store".into(),
            )
        })?),
        other => {
            return Err(Error::InvalidConfig(format!(
                "VPN_NODE_MPP_SESSION_STORE must be memory or sqlite, got {other}"
            )))
        }
    };

    let config = StreamingConfig {
        enabled,
        mode,
        chain_id,
        reserve,
        operator,
        unit_amount,
        billing_interval_seconds,
        suggested_reserve,
        min_voucher_delta,
        grace_period_seconds,
        close_signer,
        store,
    };
    validate_streaming_config(&config)?;
    Ok(config)
}

fn parse_address(name: &str, value: &str) -> Result<Address> {
    value
        .parse()
        .map_err(|_| Error::InvalidConfig(format!("{name} is not a valid address")))
}

fn validate_streaming_config(config: &StreamingConfig) -> Result<()> {
    if !config.enabled {
        return Ok(());
    }
    if config.chain_id == 0 {
        return Err(Error::InvalidConfig(
            "VPN_NODE_MPP_CHAIN_ID must be greater than zero".into(),
        ));
    }
    if config.reserve == Address::ZERO || config.operator == Address::ZERO {
        return Err(Error::InvalidConfig(
            "streaming reserve and operator addresses must be non-zero".into(),
        ));
    }
    if config.unit_amount == 0 || config.unit_amount > PRECOMPILE_MAX_CUMULATIVE_AMOUNT {
        return Err(Error::InvalidConfig(
            "streaming unit amount must fit Tempo's uint96 reserve amount".into(),
        ));
    }
    if config.suggested_reserve <= config.unit_amount
        || config.suggested_reserve > PRECOMPILE_MAX_CUMULATIVE_AMOUNT
    {
        return Err(Error::InvalidConfig(
            "suggested reserve must cover more than one unit and fit uint96".into(),
        ));
    }
    if config.min_voucher_delta > PRECOMPILE_MAX_CUMULATIVE_AMOUNT {
        return Err(Error::InvalidConfig(
            "minimum voucher delta must fit uint96".into(),
        ));
    }
    if config.billing_interval_seconds == 0 || config.grace_period_seconds == 0 {
        return Err(Error::InvalidConfig(
            "billing interval and grace period must be greater than zero".into(),
        ));
    }
    if config.close_signer.is_none() {
        return Err(Error::InvalidConfig(
            "VPN_NODE_MPP_SESSION_CLOSE_PRIVATE_KEY is required when streaming is enabled".into(),
        ));
    }
    if config.mode == StreamingMode::Production
        && !matches!(config.store, ChannelStoreConfig::Sqlite(_))
    {
        return Err(Error::InvalidConfig(
            "production streaming requires VPN_NODE_MPP_SESSION_STORE=sqlite".into(),
        ));
    }
    Ok(())
}

fn validate_production_payment_identity(
    streaming: &StreamingConfig,
    rpc_url: &str,
    realm: &str,
    currency: &str,
    recipient: &str,
) -> Result<()> {
    if !streaming.enabled || streaming.mode != StreamingMode::Production {
        return Ok(());
    }
    if streaming.chain_id != 4217
        || rpc_url == DEFAULT_MPP_RPC_URL
        || rpc_url.to_ascii_lowercase().contains("moderato")
        || realm == DEFAULT_MPP_REALM
        || currency.eq_ignore_ascii_case(DEFAULT_MPP_PAYMENT_CURRENCY)
        || recipient.eq_ignore_ascii_case(DEFAULT_MPP_PAYMENT_RECIPIENT)
    {
        return Err(Error::InvalidConfig(
            "production streaming requires explicit Tempo mainnet RPC, chain, realm, currency, and recipient settings".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod production_payment_tests {
    use super::*;

    fn production_streaming() -> StreamingConfig {
        StreamingConfig {
            enabled: true,
            mode: StreamingMode::Production,
            chain_id: 4217,
            reserve: "0x4d50500000000000000000000000000000000000"
                .parse()
                .unwrap(),
            operator: "0x0000000000000000000000000000000000000001"
                .parse()
                .unwrap(),
            unit_amount: 10_000,
            billing_interval_seconds: 60,
            suggested_reserve: 100_000,
            min_voucher_delta: 10_000,
            grace_period_seconds: 30,
            close_signer: None,
            store: ChannelStoreConfig::Sqlite("/tmp/session.sqlite".into()),
        }
    }

    #[test]
    fn production_rejects_development_payment_identity() {
        let config = production_streaming();
        let error = validate_production_payment_identity(
            &config,
            DEFAULT_MPP_RPC_URL,
            DEFAULT_MPP_REALM,
            DEFAULT_MPP_PAYMENT_CURRENCY,
            DEFAULT_MPP_PAYMENT_RECIPIENT,
        )
        .unwrap_err();
        assert!(error.to_string().contains("explicit Tempo mainnet"));
    }

    #[test]
    fn production_accepts_explicit_mainnet_payment_identity() {
        let config = production_streaming();
        validate_production_payment_identity(
            &config,
            "https://rpc.tempo.xyz",
            "tempvpn.xyz",
            "0x20c000000000000000000000b9537d11c60e8b50",
            "0x59E5aa2A081FB9F56FE9ae57b7688A5884d74dDC",
        )
        .unwrap();
    }
}

fn env_var(name: &'static str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn env_or_optional(name: &'static str, value: Option<String>) -> Option<String> {
    env::var(name).ok().or(value)
}

fn env_or_required(name: &'static str, value: Option<String>) -> Result<String> {
    if let Ok(value) = env::var(name) {
        if !value.is_empty() {
            return Ok(value);
        }
    }
    value
        .filter(|value| !value.is_empty())
        .ok_or(Error::MissingConfig(name))
}

fn env_path_required(name: &'static str, value: Option<PathBuf>) -> Result<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or(value)
        .ok_or(Error::MissingConfig(name))
}

fn env_or_default(name: &'static str, value: Option<String>, default: &str) -> String {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .or(value)
        .unwrap_or_else(|| default.to_string())
}

fn env_or<T>(name: &'static str, value: Option<T>, default: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    if let Ok(raw) = env::var(name) {
        if !raw.is_empty() {
            return raw
                .parse()
                .map_err(|_| Error::InvalidConfig(format!("invalid environment variable {name}")));
        }
    }
    if let Some(value) = value {
        return Ok(value);
    }
    default
        .parse()
        .map_err(|_| Error::InvalidConfig(format!("invalid default value for {name}")))
}
