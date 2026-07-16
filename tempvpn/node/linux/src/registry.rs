use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::{info, warn};

use crate::{
    config::Config,
    error::{Error, Result},
};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NodeAdvertisement {
    pub id: String,
    pub name: String,
    pub region: String,
    pub api_url: String,
    pub wireguard_endpoint: String,
    pub expected_exit_ip: String,
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
}

pub fn advertisement(config: &Config) -> NodeAdvertisement {
    NodeAdvertisement {
        id: config.node_id.clone(),
        name: config.node_name.clone(),
        region: config.node_region.clone(),
        api_url: config.public_api_url.trim_end_matches('/').to_string(),
        wireguard_endpoint: config.endpoint.clone(),
        expected_exit_ip: config.expected_exit_ip.clone(),
    }
}

pub fn spawn_registration(config: Config) -> Option<JoinHandle<()>> {
    let registry_url = config.registry_url.clone()?;
    let token = config.registry_token.clone()?;
    let node = advertisement(&config);
    Some(tokio::spawn(async move {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/registry/nodes/{}",
            registry_url.trim_end_matches('/'),
            node.id
        );
        let mut backoff = 1u64;
        loop {
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
}
