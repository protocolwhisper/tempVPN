use std::{path::Path, sync::Arc, time::Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{fs, process::Command, sync::Semaphore, task::JoinSet, time::Duration};
use tracing::{debug, info, warn};

use crate::{
    config::Config,
    error::{Error, Result},
    helpers::filter_nodes,
};

const MAX_CONCURRENT_PROBES: usize = 8;

#[derive(Debug, Clone)]
pub struct NodeClient {
    base_url: String,
    mppx_command: String,
    mppx_account: Option<String>,
    mppx_config: Option<std::path::PathBuf>,
    mppx_network: Option<String>,
    mppx_rpc_url: Option<String>,
    http: reqwest::Client,
    selected_node: Option<Node>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub region: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdivision_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepting_sessions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_slots: Option<usize>,
    pub api_url: String,
    pub wireguard_endpoint: String,
    pub expected_exit_ip: String,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct DiscoveryFilters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
}

impl DiscoveryFilters {
    pub fn new(country: Option<&str>, city: Option<&str>, region: Option<&str>) -> Result<Self> {
        let country = country
            .map(|value| value.trim().to_ascii_uppercase())
            .filter(|value| !value.is_empty());
        if country.as_ref().is_some_and(|country| {
            country.len() != 2 || !country.bytes().all(|byte| byte.is_ascii_alphabetic())
        }) {
            return Err(Error::InvalidConfig(
                "--country must be an ISO 3166-1 alpha-2 code such as DE".into(),
            ));
        }
        Ok(Self {
            country,
            city: normalize_filter("--city", city)?,
            region: normalize_filter("--region", region)?,
            available: Some(true),
        })
    }
}

fn normalize_filter(name: &str, value: Option<&str>) -> Result<Option<String>> {
    value
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                Err(Error::InvalidConfig(format!(
                    "{name} cannot be empty when supplied"
                )))
            } else {
                Ok(value.to_string())
            }
        })
        .transpose()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CatalogCache {
    fetched_at: DateTime<Utc>,
    #[serde(default)]
    filters: DiscoveryFilters,
    nodes: Vec<Node>,
}

#[derive(Debug, Clone, Deserialize)]
struct HealthResponse {
    status: String,
    #[serde(default)]
    accepting_sessions: Option<bool>,
    #[serde(default)]
    available_slots: Option<usize>,
}

#[derive(Debug, Serialize)]
struct CreateSessionRequest {
    node_id: String,
    duration_seconds: u64,
}

#[derive(Debug, Serialize)]
struct ConnectSessionRequest<'a> {
    node_id: &'a str,
    client_public_key: &'a str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreatedSession {
    pub session_id: String,
    #[serde(default)]
    pub node_url: Option<String>,
    #[serde(alias = "grace_deadline")]
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
            selected_node: None,
        }
    }

    pub async fn select(
        config: &Config,
        filters: &DiscoveryFilters,
        node_id: Option<&str>,
        node_url: Option<&str>,
    ) -> Result<Self> {
        if node_id.is_some() || node_url.is_some() {
            let directory = Self::new(config);
            let nodes = directory.nodes(filters).await?;
            let node = nodes
                .into_iter()
                .find(|node| {
                    node_id.is_some_and(|wanted| node.id == wanted)
                        || node_url.is_some_and(|wanted| {
                            node.api_url.trim_end_matches('/') == wanted.trim_end_matches('/')
                        })
                })
                .ok_or_else(|| {
                    Error::InvalidConfig(format!(
                        "selected node is not in the eligible registry catalog"
                    ))
                })?;
            median_health_rtt(&directory.http, &node.api_url).await?;
            return Ok(Self::for_selected_node(node, config));
        }

        let directory = Self::new(config);
        let (nodes, cached_fallback) = match directory.nodes(filters).await {
            Ok(nodes) => {
                if let Err(error) = write_cache(&config.catalog_cache_file, &nodes, filters).await {
                    warn!(%error, "could not update node catalog cache");
                }
                (nodes, false)
            }
            Err(error) => {
                warn!(%error, "registry unavailable; trying cached catalog");
                (
                    read_cache(
                        &config.catalog_cache_file,
                        config.catalog_cache_ttl_seconds,
                        filters,
                    )
                    .await?,
                    true,
                )
            }
        };

        let nodes = filter_nodes(nodes, filters);
        let ranked = rank_nodes(directory.http.clone(), nodes).await;

        let Some((rtt, node)) = ranked.into_iter().next() else {
            return Err(Error::NoHealthyNodes);
        };
        info!(
            node = node.id,
            region = node.region,
            country = node.country_code.as_deref().unwrap_or("unknown"),
            rtt_ms = rtt,
            cached = cached_fallback,
            "selected fastest VPN node"
        );
        Ok(Self::for_selected_node(node, config))
    }

    fn for_selected_node(node: Node, config: &Config) -> Self {
        let mut client = Self::for_base_url(config.node_url.clone(), config);
        client.selected_node = Some(node);
        client
    }

    pub async fn nodes(&self, filters: &DiscoveryFilters) -> Result<Vec<Node>> {
        let url = format!("{}/nodes", self.base_url);
        let mut query = Vec::new();
        if let Some(country) = &filters.country {
            query.push(("country", country.clone()));
        }
        if let Some(city) = &filters.city {
            query.push(("city", city.clone()));
        }
        if let Some(region) = &filters.region {
            query.push(("region", region.clone()));
        }
        if let Some(available) = filters.available {
            query.push(("available", available.to_string()));
        }
        let url = reqwest::Url::parse_with_params(&url, &query)
            .map_err(|error| Error::InvalidConfig(format!("invalid registry URL: {error}")))?;
        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.json::<Vec<Node>>().await?)
    }

    pub async fn create_session(&self, duration_seconds: u64) -> Result<CreatedSession> {
        let node_id = self
            .selected_node
            .as_ref()
            .ok_or_else(|| Error::InvalidConfig("select a registry node before purchase".into()))?
            .id
            .clone();
        let url = format!("{}/sessions", self.base_url);
        let body = serde_json::to_string(&CreateSessionRequest {
            node_id,
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
        let node_id = self
            .selected_node
            .as_ref()
            .ok_or_else(|| Error::InvalidConfig("select a registry node before connect".into()))?
            .id
            .as_str();
        let url = format!("{}/sessions/{session_id}/connect", self.base_url);
        let response = self
            .http
            .post(url)
            .json(&ConnectSessionRequest {
                node_id,
                client_public_key: public_key,
            })
            .send()
            .await?
            .error_for_status()?;
        let session = response.json::<ServerSession>().await?;
        session.try_into()
    }

    pub async fn pause_session(&self, session_id: &str) -> Result<CreatedSession> {
        let url = format!("{}/sessions/{session_id}/pause", self.base_url);
        let response = self.http.post(url).send().await?.error_for_status()?;
        Ok(response.json::<CreatedSession>().await?)
    }

    pub async fn heartbeat(&self, session_id: &str) -> Result<CreatedSession> {
        let url = format!("{}/sessions/{session_id}/heartbeat", self.base_url);
        let response = self.http.post(url).send().await?.error_for_status()?;
        Ok(response.json::<CreatedSession>().await?)
    }

    pub async fn session_status(&self, session_id: &str) -> Result<CreatedSession> {
        let url = format!("{}/sessions/{session_id}/status", self.base_url);
        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.json::<CreatedSession>().await?)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn selected_node(&self) -> Option<&Node> {
        self.selected_node.as_ref()
    }

    pub async fn check_available(&self) -> Result<()> {
        self.ensure_available(true).await
    }

    async fn ensure_available(&self, require_snapshot: bool) -> Result<()> {
        let health = fetch_health(&self.http, &self.base_url).await?;
        validate_health_for_payment(&health, require_snapshot)
    }
}

async fn rank_nodes(http: reqwest::Client, nodes: Vec<Node>) -> Vec<(u128, Node)> {
    let mut probes = JoinSet::new();
    let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_PROBES));
    for node in nodes {
        let http = http.clone();
        let permits = permits.clone();
        probes.spawn(async move {
            let _permit = permits.acquire_owned().await.ok();
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
    ranked.sort_by(|(left_rtt, left_node), (right_rtt, right_node)| {
        left_rtt
            .cmp(right_rtt)
            .then_with(|| left_node.id.cmp(&right_node.id))
    });
    ranked
}

async fn write_cache(path: &Path, nodes: &[Node], filters: &DiscoveryFilters) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let cache = CatalogCache {
        fetched_at: Utc::now(),
        filters: filters.clone(),
        nodes: nodes.to_vec(),
    };
    fs::write(path, serde_json::to_vec(&cache)?).await?;
    Ok(())
}

async fn read_cache(
    path: &Path,
    ttl_seconds: u64,
    filters: &DiscoveryFilters,
) -> Result<Vec<Node>> {
    let cache: CatalogCache = serde_json::from_slice(&fs::read(path).await?)?;
    if Utc::now() - cache.fetched_at > chrono::Duration::seconds(ttl_seconds as i64) {
        return Err(Error::NoHealthyNodes);
    }
    if cache.filters != *filters {
        return Err(Error::NoHealthyNodes);
    }
    Ok(cache.nodes)
}

async fn fetch_health(http: &reqwest::Client, api_url: &str) -> Result<HealthResponse> {
    let url = format!("{}/health", api_url.trim_end_matches('/'));
    Ok(http
        .get(url)
        .timeout(Duration::from_secs(2))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

fn validate_health_for_payment(health: &HealthResponse, require_snapshot: bool) -> Result<()> {
    if health.status != "ok" {
        return Err(Error::NodeUnhealthy);
    }
    match (health.accepting_sessions, health.available_slots) {
        (Some(true), Some(slots)) if slots > 0 => Ok(()),
        (None, None) if !require_snapshot => Ok(()),
        _ => Err(Error::NodeUnavailable),
    }
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
    use std::{net::SocketAddr, path::PathBuf};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    fn node(id: &str, region: &str) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            region: region.into(),
            country_code: None,
            subdivision_code: None,
            city: None,
            accepting_sessions: Some(true),
            available_slots: Some(2),
            api_url: format!("https://{id}.example"),
            wireguard_endpoint: "192.0.2.1:51820".into(),
            expected_exit_ip: "192.0.2.1".into(),
            lease_expires_at: Utc::now() + chrono::Duration::seconds(90),
        }
    }

    fn test_config(node_url: String) -> Config {
        Config {
            node_url,
            catalog_cache_file: PathBuf::from("/tmp/test-tempvpn-cache.json"),
            catalog_cache_ttl_seconds: 60,
            mppx_command: "/definitely/not/an/mppx-binary".into(),
            mppx_account: Some("main".into()),
            mppx_config: None,
            mppx_network: None,
            mppx_rpc_url: None,
            proxy_addr: "127.0.0.1:1080".parse::<SocketAddr>().unwrap(),
            status_file: PathBuf::from("/tmp/test-tempvpn-status.json"),
            session_store_file: PathBuf::from("/tmp/test-tempvpn-sessions.json"),
            wg_quick_command: "wg-quick".into(),
            wg_command: "wg".into(),
            interface_name: "testwg0".into(),
            expected_exit_ip: None,
        }
    }

    async fn health_server(
        body: &'static str,
        delay: std::time::Duration,
        requests: usize,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = [0u8; 2048];
                let _ = stream.read(&mut buffer).await.unwrap();
                tokio::time::sleep(delay).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn cache_is_rejected_after_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nodes.json");
        let filters = DiscoveryFilters::new(Some("DE"), None, None).unwrap();
        write_cache(&path, &[node("a", "eu")], &filters)
            .await
            .unwrap();
        assert_eq!(read_cache(&path, 60, &filters).await.unwrap().len(), 1);
        assert!(read_cache(&path, 0, &filters).await.is_err());
        let france = DiscoveryFilters::new(Some("FR"), None, None).unwrap();
        assert!(read_cache(&path, 60, &france).await.is_err());
    }

    #[test]
    fn discovery_filters_normalize_country_and_reject_names() {
        let filters = DiscoveryFilters::new(Some(" de "), Some(" Frankfurt "), None).unwrap();
        assert_eq!(filters.country.as_deref(), Some("DE"));
        assert_eq!(filters.city.as_deref(), Some("Frankfurt"));
        assert_eq!(filters.available, Some(true));
        assert!(DiscoveryFilters::new(Some("Germany"), None, None).is_err());
    }

    #[test]
    fn payment_health_requires_positive_explicit_capacity_after_discovery() {
        let ready = HealthResponse {
            status: "ok".into(),
            accepting_sessions: Some(true),
            available_slots: Some(1),
        };
        assert!(validate_health_for_payment(&ready, true).is_ok());

        for unavailable in [
            HealthResponse {
                status: "ok".into(),
                accepting_sessions: Some(false),
                available_slots: Some(4),
            },
            HealthResponse {
                status: "ok".into(),
                accepting_sessions: Some(true),
                available_slots: Some(0),
            },
            HealthResponse {
                status: "ok".into(),
                accepting_sessions: None,
                available_slots: None,
            },
        ] {
            assert!(matches!(
                validate_health_for_payment(&unavailable, true),
                Err(Error::NodeUnavailable)
            ));
        }
    }

    #[test]
    fn deterministic_latency_ties_use_node_id() {
        let mut ranked = [(10, node("z", "eu")), (10, node("a", "eu"))];
        ranked.sort_by(|(left_rtt, left_node), (right_rtt, right_node)| {
            left_rtt
                .cmp(right_rtt)
                .then_with(|| left_node.id.cmp(&right_node.id))
        });
        assert_eq!(ranked[0].1.id, "a");
    }

    #[tokio::test]
    async fn latency_ranking_uses_client_observed_median() {
        let fast_url = health_server("{}", std::time::Duration::from_millis(1), 3).await;
        let slow_url = health_server("{}", std::time::Duration::from_millis(20), 3).await;
        let mut fast = node("fast", "eu");
        fast.api_url = fast_url;
        let mut slow = node("slow", "eu");
        slow.api_url = slow_url;

        let ranked = rank_nodes(reqwest::Client::new(), vec![slow, fast]).await;
        assert_eq!(ranked[0].1.id, "fast");
    }

    #[tokio::test]
    async fn registry_owns_final_capacity_check_before_payment() {
        let url = health_server(
            r#"{"status":"ok","accepting_sessions":false,"available_slots":3}"#,
            std::time::Duration::ZERO,
            1,
        )
        .await;
        let config = test_config(url.clone());
        let mut selected = node("draining", "eu");
        selected.api_url = url;
        let client = NodeClient::for_selected_node(selected, &config);

        let error = client.create_session(60).await.unwrap_err();
        assert!(matches!(error, Error::Io(_)));
    }

    #[test]
    fn portable_session_accepts_coordinator_deadline_and_missing_node_url() {
        let session: CreatedSession = serde_json::from_str(
            r#"{"session_id":"sess_portable","logical_node":"madrid","state":"paused","total_seconds":120,"remaining_seconds":120,"grace_deadline":"2030-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(session.session_id, "sess_portable");
        assert!(session.node_url.is_none());
        assert_eq!(session.total_seconds, 120);
    }
}
