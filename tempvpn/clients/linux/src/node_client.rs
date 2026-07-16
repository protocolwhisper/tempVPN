use std::{path::Path, time::Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{fs, process::Command, task::JoinSet, time::Duration};
use tracing::{debug, info, warn};

use crate::{
    config::Config,
    error::{Error, Result},
    helpers::filter_region,
};

#[derive(Debug, Clone)]
pub struct NodeClient {
    base_url: String,
    mppx_command: String,
    mppx_account: Option<String>,
    mppx_config: Option<std::path::PathBuf>,
    mppx_network: Option<String>,
    mppx_rpc_url: Option<String>,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub region: String,
    pub api_url: String,
    pub wireguard_endpoint: String,
    pub expected_exit_ip: String,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CatalogCache {
    fetched_at: DateTime<Utc>,
    nodes: Vec<Node>,
}

#[derive(Debug, Serialize)]
struct CreateSessionRequest {
    duration_seconds: u64,
}

#[derive(Debug, Serialize)]
struct ConnectSessionRequest<'a> {
    client_public_key: &'a str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreatedSession {
    pub session_id: String,
    pub node_url: String,
    pub not_after: DateTime<Utc>,
    pub total_seconds: u64,
    pub remaining_seconds: u64,
    pub state: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    pub session_id: String,
    pub node_url: String,
    pub assigned_ip: String,
    pub server_public_key: String,
    pub endpoint: String,
    pub expected_exit_ip: String,
    pub not_after: DateTime<Utc>,
    pub remaining_seconds: u64,
    pub state: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerSession {
    session_id: String,
    node_url: String,
    assigned_ip: Option<String>,
    server_public_key: String,
    endpoint: String,
    expected_exit_ip: String,
    not_after: DateTime<Utc>,
    remaining_seconds: u64,
    state: String,
}

impl TryFrom<ServerSession> for Session {
    type Error = Error;

    fn try_from(value: ServerSession) -> Result<Self> {
        Ok(Self {
            session_id: value.session_id,
            node_url: value.node_url,
            assigned_ip: value.assigned_ip.ok_or(Error::MissingAssignedIp)?,
            server_public_key: value.server_public_key,
            endpoint: value.endpoint,
            expected_exit_ip: value.expected_exit_ip,
            not_after: value.not_after,
            remaining_seconds: value.remaining_seconds,
            state: value.state,
        })
    }
}

impl NodeClient {
    pub fn new(config: &Config) -> Self {
        Self::for_base_url(config.node_url.clone(), config)
    }

    pub fn for_base_url(base_url: String, config: &Config) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            mppx_command: config.mppx_command.clone(),
            mppx_account: config.mppx_account.clone(),
            mppx_config: config.mppx_config.clone(),
            mppx_network: config.mppx_network.clone(),
            mppx_rpc_url: config.mppx_rpc_url.clone(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn select(
        config: &Config,
        region: Option<&str>,
        node_url: Option<&str>,
    ) -> Result<Self> {
        if let Some(node_url) = node_url {
            let node = Self::for_base_url(node_url.to_string(), config);
            median_health_rtt(&node.http, node.base_url()).await?;
            return Ok(node);
        }

        let directory = Self::new(config);
        let (nodes, cached_fallback) = match directory.nodes().await {
            Ok(nodes) => {
                if let Err(error) = write_cache(&config.catalog_cache_file, &nodes).await {
                    warn!(%error, "could not update node catalog cache");
                }
                (nodes, false)
            }
            Err(error) => {
                warn!(%error, "registry unavailable; trying cached catalog");
                (
                    read_cache(&config.catalog_cache_file, config.catalog_cache_ttl_seconds)
                        .await?,
                    true,
                )
            }
        };

        let nodes = filter_region(nodes, region);
        let ranked = rank_nodes(directory.http.clone(), nodes).await;

        let Some((rtt, node)) = ranked.into_iter().next() else {
            return Err(Error::NoHealthyNodes);
        };
        info!(
            node = node.id,
            region = node.region,
            rtt_ms = rtt,
            cached = cached_fallback,
            "selected fastest VPN node"
        );
        Ok(Self::for_base_url(node.api_url, config))
    }

    pub async fn nodes(&self) -> Result<Vec<Node>> {
        let url = format!("{}/nodes", self.base_url);
        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.json::<Vec<Node>>().await?)
    }

    pub async fn create_session(&self, duration_seconds: u64) -> Result<CreatedSession> {
        let url = format!("{}/sessions", self.base_url);
        let body = serde_json::to_string(&CreateSessionRequest { duration_seconds })?;

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
            url, "creating paid VPN usage-balance session with mppx"
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

    pub async fn connect_session(&self, session_id: &str, public_key: &str) -> Result<Session> {
        let url = format!("{}/sessions/{session_id}/connect", self.base_url);
        let response = self
            .http
            .post(url)
            .json(&ConnectSessionRequest {
                client_public_key: public_key,
            })
            .send()
            .await?
            .error_for_status()?;
        let session = response.json::<ServerSession>().await?;
        session.try_into()
    }

    pub async fn pause_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/sessions/{session_id}/pause", self.base_url);
        self.http.post(url).send().await?.error_for_status()?;
        Ok(())
    }

    pub async fn heartbeat(&self, session_id: &str) -> Result<CreatedSession> {
        let url = format!("{}/sessions/{session_id}/heartbeat", self.base_url);
        let response = self.http.post(url).send().await?.error_for_status()?;
        Ok(response.json::<CreatedSession>().await?)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

async fn rank_nodes(http: reqwest::Client, nodes: Vec<Node>) -> Vec<(u128, Node)> {
    let mut probes = JoinSet::new();
    for node in nodes {
        let http = http.clone();
        probes.spawn(async move {
            let result = median_health_rtt(&http, &node.api_url).await;
            (node, result)
        });
    }
    let mut ranked = Vec::new();
    while let Some(result) = probes.join_next().await {
        if let Ok((node, result)) = result {
            match result {
                Ok(rtt) => ranked.push((rtt, node)),
                Err(error) => warn!(node = node.id, %error, "node health probe failed"),
            }
        }
    }
    ranked.sort_by_key(|(rtt, _)| *rtt);
    ranked
}

async fn write_cache(path: &Path, nodes: &[Node]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let cache = CatalogCache {
        fetched_at: Utc::now(),
        nodes: nodes.to_vec(),
    };
    fs::write(path, serde_json::to_vec(&cache)?).await?;
    Ok(())
}

async fn read_cache(path: &Path, ttl_seconds: u64) -> Result<Vec<Node>> {
    let cache: CatalogCache = serde_json::from_slice(&fs::read(path).await?)?;
    if Utc::now() - cache.fetched_at > chrono::Duration::seconds(ttl_seconds as i64) {
        return Err(Error::NoHealthyNodes);
    }
    Ok(cache.nodes)
}

async fn median_health_rtt(http: &reqwest::Client, api_url: &str) -> Result<u128> {
    let mut samples = Vec::new();
    for _ in 0..3 {
        let url = format!("{}/health", api_url.trim_end_matches('/'));
        let start = Instant::now();
        http.get(url)
            .timeout(Duration::from_secs(2))
            .send()
            .await?
            .error_for_status()?;
        samples.push(start.elapsed().as_millis());
    }
    samples.sort_unstable();
    Ok(samples[samples.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, region: &str) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            region: region.into(),
            api_url: format!("https://{id}.example"),
            wireguard_endpoint: "192.0.2.1:51820".into(),
            expected_exit_ip: "192.0.2.1".into(),
            lease_expires_at: Utc::now() + chrono::Duration::seconds(90),
        }
    }

    #[tokio::test]
    async fn cache_is_rejected_after_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nodes.json");
        write_cache(&path, &[node("a", "eu")]).await.unwrap();
        assert_eq!(read_cache(&path, 60).await.unwrap().len(), 1);
        assert!(read_cache(&path, 0).await.is_err());
    }
}
