use std::{env, net::SocketAddr, path::PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

const DEFAULT_MPPX_COMMAND: &str = "mppx";

#[derive(Debug, Clone)]
pub struct Config {
    pub node_url: String,
    pub catalog_cache_file: PathBuf,
    pub catalog_cache_ttl_seconds: u64,
    pub mppx_command: String,
    pub mppx_account: Option<String>,
    pub mppx_config: Option<PathBuf>,
    pub mppx_network: Option<String>,
    pub mppx_rpc_url: Option<String>,
    pub proxy_addr: SocketAddr,
    pub status_file: PathBuf,
    pub session_store_file: PathBuf,
    pub wg_quick_command: String,
    pub wg_command: String,
    pub interface_name: String,
    pub expected_exit_ip: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    node_url: Option<String>,
    registry_url: Option<String>,
    catalog_cache_file: Option<PathBuf>,
    catalog_cache_ttl_seconds: Option<u64>,
    mppx_command: Option<String>,
    mppx_account: Option<String>,
    mppx_config: Option<PathBuf>,
    mppx_network: Option<String>,
    mppx_rpc_url: Option<String>,
    proxy_addr: Option<SocketAddr>,
    status_file: Option<PathBuf>,
    session_store_file: Option<PathBuf>,
    wg_quick_command: Option<String>,
    wg_command: Option<String>,
    interface_name: Option<String>,
    expected_exit_ip: Option<String>,
}

impl Config {
    pub async fn load(path: Option<PathBuf>) -> Result<Self> {
        let file = match path {
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

        let proxy_addr = env_or("VPN_CLIENT_PROXY_ADDR", file.proxy_addr, "127.0.0.1:1080")?;
        if !proxy_addr.ip().is_loopback() {
            return Err(Error::ProxyMustBeLoopback(proxy_addr));
        }

        let registry_url = env_var("VPN_CLIENT_REGISTRY_URL")
            .or(file.registry_url)
            .or_else(|| env_var("VPN_CLIENT_NODE_URL"))
            .or(file.node_url)
            .ok_or_else(|| {
                Error::InvalidConfig(
                    "registry_url or VPN_CLIENT_REGISTRY_URL is required".to_string(),
                )
            })?;

        Ok(Self {
            node_url: registry_url,
            catalog_cache_file: env_or(
                "VPN_CLIENT_CATALOG_CACHE_FILE",
                file.catalog_cache_file,
                "/tmp/tempvpn-nodes.json",
            )?,
            catalog_cache_ttl_seconds: env_or(
                "VPN_CLIENT_CATALOG_CACHE_TTL_SECONDS",
                file.catalog_cache_ttl_seconds,
                "86400",
            )?,
            mppx_command: env_or_default(
                "VPN_CLIENT_MPPX_COMMAND",
                file.mppx_command,
                DEFAULT_MPPX_COMMAND,
            ),
            mppx_account: env_var("MPPX_ACCOUNT").or(file.mppx_account),
            mppx_config: env_or_optional("MPPX_CONFIG", file.mppx_config)?,
            mppx_network: env_var("MPPX_NETWORK").or(file.mppx_network),
            mppx_rpc_url: env_var("MPPX_RPC_URL").or(file.mppx_rpc_url),
            proxy_addr,
            status_file: env_or(
                "VPN_CLIENT_STATUS_FILE",
                file.status_file,
                "/tmp/vpn-client-status.json",
            )?,
            session_store_file: env_or_optional(
                "VPN_CLIENT_SESSION_STORE_FILE",
                file.session_store_file,
            )?
            .unwrap_or(default_session_store_file()?),
            wg_quick_command: env_or_default(
                "VPN_CLIENT_WG_QUICK_COMMAND",
                file.wg_quick_command,
                "wg-quick",
            ),
            wg_command: env_or_default("VPN_CLIENT_WG_COMMAND", file.wg_command, "wg"),
            interface_name: env_or_default(
                "VPN_CLIENT_INTERFACE_NAME",
                file.interface_name,
                "vpnclient0",
            ),
            expected_exit_ip: env::var("VPN_CLIENT_EXPECTED_EXIT_IP")
                .ok()
                .filter(|value| !value.is_empty())
                .or(file.expected_exit_ip),
        })
    }
}

fn default_session_store_file() -> Result<PathBuf> {
    if let Some(state) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(state).join("tempvpn/sessions.json"));
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home).join(".local/state/tempvpn/sessions.json"));
    }
    Err(Error::InvalidConfig(
        "set VPN_CLIENT_SESSION_STORE_FILE when no user state directory is available".into(),
    ))
}

fn env_var(name: &'static str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn env_or_optional<T>(name: &'static str, value: Option<T>) -> Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    if let Ok(raw) = env::var(name) {
        if !raw.is_empty() {
            return raw
                .parse()
                .map(Some)
                .map_err(|_| Error::InvalidConfig(format!("invalid environment variable {name}")));
        }
    }
    Ok(value)
}

fn env_or_default(name: &'static str, value: Option<String>, default: &str) -> String {
    if let Ok(value) = env::var(name) {
        if !value.is_empty() {
            return value;
        }
    }
    value
        .filter(|value| !value.is_empty())
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
