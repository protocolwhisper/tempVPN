use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::debug;

use crate::{
    config::Config,
    error::{Error, Result},
};

#[derive(Debug, Clone)]
pub struct NodeClient {
    base_url: String,
    mppx_command: String,
    mppx_account: Option<String>,
    mppx_config: Option<std::path::PathBuf>,
    mppx_network: Option<String>,
    mppx_rpc_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateSessionRequest<'a> {
    client_public_key: &'a str,
    duration_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    pub session_id: String,
    pub assigned_ip: String,
    pub server_public_key: String,
    pub endpoint: String,
    pub expires_at: DateTime<Utc>,
}

impl NodeClient {
    pub fn new(config: &Config) -> Self {
        Self {
            base_url: config.node_url.trim_end_matches('/').to_string(),
            mppx_command: config.mppx_command.clone(),
            mppx_account: config.mppx_account.clone(),
            mppx_config: config.mppx_config.clone(),
            mppx_network: config.mppx_network.clone(),
            mppx_rpc_url: config.mppx_rpc_url.clone(),
        }
    }

    pub async fn create_session(&self, public_key: &str, duration_seconds: u64) -> Result<Session> {
        let url = format!("{}/sessions", self.base_url);
        let body = serde_json::to_string(&CreateSessionRequest {
            client_public_key: public_key,
            duration_seconds,
        })?;

        let mut command = Command::new(&self.mppx_command);
        command
            .arg(&url)
            .arg("--json-body")
            .arg(body)
            .arg("--silent");

        if let Some(account) = &self.mppx_account {
            command.arg("--account").arg(account);
        }
        if let Some(config) = &self.mppx_config {
            command.arg("--config").arg(config);
        }
        if let Some(network) = &self.mppx_network {
            command.arg("--network").arg(network);
        }
        if let Some(rpc_url) = &self.mppx_rpc_url {
            command.arg("--rpc-url").arg(rpc_url);
        }

        debug!(
            program = self.mppx_command,
            url, "creating paid VPN session with mppx"
        );
        let output = command.output().await.map_err(Error::Io)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(Error::CommandFailed {
                program: self.mppx_command.clone(),
                stderr: if stdout.is_empty() {
                    stderr
                } else if stderr.is_empty() {
                    stdout
                } else {
                    format!("{stderr}\n{stdout}")
                },
            });
        }

        Ok(serde_json::from_slice(&output.stdout)?)
    }
}
