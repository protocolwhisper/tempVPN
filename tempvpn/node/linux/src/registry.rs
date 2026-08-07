use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::{info, warn};

use crate::{
    config::Config,
    error::{Error, Result},
    location::{normalize_country_code, normalize_optional_text},
    sessions::Sessions,
};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NodeAdvertisement {
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeFilters {
    pub country_code: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub available: Option<bool>,
}

impl NodeFilters {
    pub fn normalize(
        country_code: Option<&str>,
        city: Option<&str>,
        region: Option<&str>,
        available: Option<bool>,
    ) -> Result<Self> {
        Ok(Self {
            country_code: normalize_country_code(country_code)
                .map_err(Error::InvalidRegistryAdvertisement)?,
            city: normalize_optional_text("city", city)
                .map_err(Error::InvalidRegistryAdvertisement)?,
            region: normalize_optional_text("region", region)
                .map_err(Error::InvalidRegistryAdvertisement)?,
            available,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RegisteredNode {
    #[serde(flatten)]
    pub node: NodeAdvertisement,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Clone, Default)]
pub struct Registry {
    nodes: Arc<RwLock<HashMap<String, RegisteredNode>>>,
}

impl Registry {
    pub async fn upsert(
        &self,
        node_id: &str,
        mut node: NodeAdvertisement,
        lease_seconds: u64,
    ) -> Result<RegisteredNode> {
        if node_id.is_empty()
            || !node_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || node.id != node_id
        {
            return Err(Error::InvalidRegistryAdvertisement(
                "node ID must match the path and contain only letters, digits, '.', '_' or '-'"
                    .into(),
            ));
        }
        node.api_url = node.api_url.trim_end_matches('/').to_string();
        node.country_code = normalize_country_code(node.country_code.as_deref())
            .map_err(Error::InvalidRegistryAdvertisement)?;
        node.subdivision_code =
            normalize_optional_text("subdivision_code", node.subdivision_code.as_deref())
                .map_err(Error::InvalidRegistryAdvertisement)?;
        node.city = normalize_optional_text("city", node.city.as_deref())
            .map_err(Error::InvalidRegistryAdvertisement)?;
        let registered = RegisteredNode {
            node,
            lease_expires_at: Utc::now() + chrono::Duration::seconds(lease_seconds.max(1) as i64),
        };
        self.nodes
            .write()
            .await
            .insert(node_id.to_string(), registered.clone());
        Ok(registered)
    }

    pub async fn remove(&self, node_id: &str) -> bool {
        self.nodes.write().await.remove(node_id).is_some()
    }

    pub async fn active(&self) -> Vec<RegisteredNode> {
        let now = Utc::now();
        let mut nodes = self.nodes.write().await;
        nodes.retain(|_, node| node.lease_expires_at > now);
        let mut active: Vec<_> = nodes.values().cloned().collect();
        active.sort_by(|a, b| a.node.id.cmp(&b.node.id));
        active
    }

    pub async fn active_filtered(&self, filters: &NodeFilters) -> Vec<RegisteredNode> {
        self.active()
            .await
            .into_iter()
            .filter(|registered| matches_filters(&registered.node, filters))
            .collect()
    }
}

fn matches_filters(node: &NodeAdvertisement, filters: &NodeFilters) -> bool {
    let matches_text = |actual: Option<&str>, requested: Option<&str>| match requested {
        Some(requested) => actual.is_some_and(|actual| actual.eq_ignore_ascii_case(requested)),
        None => true,
    };
    let is_available = node.accepting_sessions == Some(true)
        && node.available_slots.is_some_and(|slots| slots > 0);

    matches_text(
        node.country_code.as_deref(),
        filters.country_code.as_deref(),
    ) && matches_text(node.city.as_deref(), filters.city.as_deref())
        && matches_text(Some(&node.region), filters.region.as_deref())
        && filters
            .available
            .is_none_or(|requested| requested == is_available)
}

pub async fn advertisement(
    config: &Config,
    sessions: &Sessions,
    coordinated_peer_count: Option<&AtomicUsize>,
) -> NodeAdvertisement {
    let coordinated_active = coordinated_peer_count
        .map(|count| count.load(Ordering::Relaxed))
        .unwrap_or(0);
    NodeAdvertisement {
        id: config.node_id.clone(),
        name: config.node_name.clone(),
        region: config.node_region.clone(),
        country_code: config.node_country_code.clone(),
        subdivision_code: config.node_subdivision_code.clone(),
        city: config.node_city.clone(),
        accepting_sessions: Some(config.accepting_sessions),
        available_slots: Some(
            sessions
                .available_slots()
                .await
                .saturating_sub(coordinated_active),
        ),
        api_url: config.public_api_url.trim_end_matches('/').to_string(),
        wireguard_endpoint: config.endpoint.clone(),
        expected_exit_ip: config.expected_exit_ip.clone(),
    }
}

pub fn spawn_registration(
    config: Config,
    sessions: Arc<Sessions>,
    coordinated_peer_count: Option<Arc<AtomicUsize>>,
) -> Option<JoinHandle<()>> {
    let registry_url = config.registry_url.clone()?;
    let token = config.registry_token.clone()?;
    let node_id = config.node_id.clone();
    Some(tokio::spawn(async move {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/registry/nodes/{}",
            registry_url.trim_end_matches('/'),
            node_id
        );
        let mut backoff = 1u64;
        loop {
            let node = advertisement(&config, &sessions, coordinated_peer_count.as_deref()).await;
            let result = client
                .put(&url)
                .bearer_auth(&token)
                .json(&node)
                .send()
                .await;
            match result {
                Ok(response) if response.status().is_success() => {
                    info!(
                        node = node.id,
                        registry = registry_url,
                        "refreshed registry lease"
                    );
                    backoff = 1;
                    tokio::time::sleep(Duration::from_secs(config.registry_refresh_seconds.max(1)))
                        .await;
                }
                Ok(response) => {
                    warn!(node = node.id, status = %response.status(), "registry refresh rejected");
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(60);
                }
                Err(error) => {
                    warn!(node = node.id, %error, "registry unavailable; VPN service remains online");
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(60);
                }
            }
        }
    }))
}

pub async fn unregister(config: &Config) {
    let (Some(registry_url), Some(token)) = (&config.registry_url, &config.registry_token) else {
        return;
    };
    let url = format!(
        "{}/registry/nodes/{}",
        registry_url.trim_end_matches('/'),
        config.node_id
    );
    if let Err(error) = reqwest::Client::new()
        .delete(url)
        .bearer_auth(token)
        .send()
        .await
    {
        warn!(%error, "failed to remove registry lease during shutdown");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> NodeAdvertisement {
        NodeAdvertisement {
            id: id.into(),
            name: id.into(),
            region: "eu-west".into(),
            country_code: None,
            subdivision_code: None,
            city: None,
            accepting_sessions: None,
            available_slots: None,
            api_url: "https://node.example/".into(),
            wireguard_endpoint: "192.0.2.1:51820".into(),
            expected_exit_ip: "192.0.2.1".into(),
        }
    }

    #[tokio::test]
    async fn refresh_replaces_duplicate_id_and_extends_lease() {
        let registry = Registry::default();
        let first = registry.upsert("a", node("a"), 1).await.unwrap();
        let mut replacement = node("a");
        replacement.name = "replacement".into();
        let second = registry.upsert("a", replacement, 90).await.unwrap();
        assert!(second.lease_expires_at > first.lease_expires_at);
        let active = registry.active().await;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].node.name, "replacement");
    }

    #[tokio::test]
    async fn removal_and_expiry_hide_nodes() {
        let registry = Registry::default();
        registry.upsert("a", node("a"), 90).await.unwrap();
        assert!(registry.remove("a").await);
        assert!(registry.active().await.is_empty());

        let expired = RegisteredNode {
            node: node("old"),
            lease_expires_at: Utc::now() - chrono::Duration::seconds(1),
        };
        registry.nodes.write().await.insert("old".into(), expired);
        assert!(registry.active().await.is_empty());
    }

    #[tokio::test]
    async fn rejects_mismatched_ids() {
        assert!(Registry::default()
            .upsert("a", node("b"), 90)
            .await
            .is_err());
        assert!(Registry::default()
            .upsert("bad/id", node("bad/id"), 90)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn normalizes_location_and_filters_conjunctively() {
        let registry = Registry::default();
        let mut frankfurt = node("de-1");
        frankfurt.country_code = Some("de".into());
        frankfurt.city = Some(" Frankfurt ".into());
        frankfurt.accepting_sessions = Some(true);
        frankfurt.available_slots = Some(4);
        registry.upsert("de-1", frankfurt, 90).await.unwrap();

        let mut paris = node("fr-1");
        paris.country_code = Some("FR".into());
        paris.city = Some("Paris".into());
        paris.accepting_sessions = Some(true);
        paris.available_slots = Some(4);
        registry.upsert("fr-1", paris, 90).await.unwrap();

        let filters =
            NodeFilters::normalize(Some("de"), Some("frankfurt"), None, Some(true)).unwrap();
        let active = registry.active_filtered(&filters).await;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].node.id, "de-1");
        assert_eq!(active[0].node.country_code.as_deref(), Some("DE"));
        assert_eq!(active[0].node.city.as_deref(), Some("Frankfurt"));
    }

    #[tokio::test]
    async fn availability_filter_excludes_full_draining_and_legacy_nodes() {
        let registry = Registry::default();
        for (id, accepting, slots) in [
            ("ready", Some(true), Some(1)),
            ("full", Some(true), Some(0)),
            ("draining", Some(false), Some(5)),
            ("legacy", None, None),
        ] {
            let mut candidate = node(id);
            candidate.accepting_sessions = accepting;
            candidate.available_slots = slots;
            registry.upsert(id, candidate, 90).await.unwrap();
        }

        let available = registry
            .active_filtered(&NodeFilters::normalize(None, None, None, Some(true)).unwrap())
            .await;
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].node.id, "ready");
        assert_eq!(registry.active().await.len(), 4);
    }

    #[tokio::test]
    async fn country_filter_excludes_legacy_location() {
        let registry = Registry::default();
        registry.upsert("legacy", node("legacy"), 90).await.unwrap();
        let filters = NodeFilters::normalize(Some("DE"), None, None, None).unwrap();
        assert!(registry.active_filtered(&filters).await.is_empty());
    }

    #[test]
    fn rejects_invalid_or_empty_location_values() {
        assert!(NodeFilters::normalize(Some("ZZ"), None, None, None).is_err());
        assert!(NodeFilters::normalize(None, Some(" "), None, None).is_err());
    }
}
