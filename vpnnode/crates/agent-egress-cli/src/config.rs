use std::{env, net::SocketAddr, path::PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub node_url: String,
    pub admin_token: String,
    pub proxy_addr: SocketAddr,
    pub status_file: PathBuf,
    pub wg_quick_command: String,
    pub wg_command: String,
    pub interface_name: String,
    pub expected_exit_ip: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    node_url: Option<String>,
    admin_token: Option<String>,
    proxy_addr: Option<SocketAddr>,
    status_file: Option<PathBuf>,
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

        let proxy_addr = env_or("AGENT_EGRESS_PROXY_ADDR", file.proxy_addr, "127.0.0.1:1080")?;
        if !proxy_addr.ip().is_loopback() {
            return Err(Error::ProxyMustBeLoopback(proxy_addr));
        }

        Ok(Self {
            node_url: env_or_required("AGENT_EGRESS_NODE_URL", file.node_url)?,
            admin_token: env_or_required("AGENT_EGRESS_ADMIN_TOKEN", file.admin_token)?,
            proxy_addr,
            status_file: env_or(
                "AGENT_EGRESS_STATUS_FILE",
                file.status_file,
                "/tmp/agent-egress-status.json",
            )?,
            wg_quick_command: env_or_default(
                "AGENT_EGRESS_WG_QUICK_COMMAND",
                file.wg_quick_command,
                "wg-quick",
            ),
            wg_command: env_or_default("AGENT_EGRESS_WG_COMMAND", file.wg_command, "wg"),
            interface_name: env_or_default(
                "AGENT_EGRESS_INTERFACE_NAME",
                file.interface_name,
                "aegress0",
            ),
            expected_exit_ip: env::var("AGENT_EGRESS_EXPECTED_EXIT_IP")
                .ok()
                .filter(|value| !value.is_empty())
                .or(file.expected_exit_ip),
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
