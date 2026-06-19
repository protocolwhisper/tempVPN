use std::{env, net::SocketAddr, path::PathBuf};

use clap::Parser;
use serde::Deserialize;

use crate::error::{Error, Result};

const DEFAULT_MPP_RPC_URL: &str = "https://rpc.moderato.tempo.xyz";
const DEFAULT_MPP_REALM: &str = "localhost:8080";
const DEFAULT_MPP_PAYMENT_CURRENCY: &str = "0x20c0000000000000000000000000000000000000";
const DEFAULT_MPP_PAYMENT_RECIPIENT: &str = "0xB01E80a8CD7C72589f30D2004aeb60937a2150d3";

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub admin_token: String,
    pub wg_interface: String,
    pub wg_command: String,
    pub server_public_key: String,
    pub endpoint: String,
    pub tunnel_cidr: String,
    pub max_duration_seconds: u64,
    pub sweep_interval_seconds: u64,
    pub cleanup_on_shutdown: bool,
    pub mock_wg: bool,
    pub mpp_rpc_url: String,
    pub mpp_realm: String,
    pub mpp_payment_currency: String,
    pub mpp_payment_recipient: String,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    bind_addr: Option<SocketAddr>,
    admin_token: Option<String>,
    wg_interface: Option<String>,
    wg_command: Option<String>,
    server_public_key: Option<String>,
    endpoint: Option<String>,
    tunnel_cidr: Option<String>,
    max_duration_seconds: Option<u64>,
    sweep_interval_seconds: Option<u64>,
    cleanup_on_shutdown: Option<bool>,
    mock_wg: Option<bool>,
    mpp_rpc_url: Option<String>,
    mpp_realm: Option<String>,
    mpp_payment_currency: Option<String>,
    mpp_payment_recipient: Option<String>,
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

        let bind_addr = env_or("VPN_NODE_BIND_ADDR", file.bind_addr, "0.0.0.0:8080")?;
        let admin_token = env_or_required("VPN_NODE_ADMIN_TOKEN", file.admin_token)?;
        let wg_interface = env_or_default("VPN_NODE_WG_INTERFACE", file.wg_interface, "wg0");
        let wg_command = env_or_default("VPN_NODE_WG_COMMAND", file.wg_command, "wg");
        let server_public_key =
            env_or_required("VPN_NODE_SERVER_PUBLIC_KEY", file.server_public_key)?;
        let endpoint = env_or_required("VPN_NODE_ENDPOINT", file.endpoint)?;
        let tunnel_cidr = env_or_default("VPN_NODE_TUNNEL_CIDR", file.tunnel_cidr, "10.8.0.0/24");
        let max_duration_seconds = env_or(
            "VPN_NODE_MAX_DURATION_SECONDS",
            file.max_duration_seconds,
            "3600",
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

        Ok(Self {
            bind_addr,
            admin_token,
            wg_interface,
            wg_command,
            server_public_key,
            endpoint,
            tunnel_cidr,
            max_duration_seconds,
            sweep_interval_seconds,
            cleanup_on_shutdown,
            mock_wg,
            mpp_rpc_url,
            mpp_realm,
            mpp_payment_currency,
            mpp_payment_recipient,
        })
    }
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
