use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use chrono::Utc;
use tempvpn_coordinator_client::{CoordinatorClient, DesiredPeer, PeerSnapshot};
use tokio::sync::Mutex;
use tracing::{error, warn};

use crate::{error::Result, wireguard::WireGuard};

pub struct PeerReconciler {
    coordinator: Arc<CoordinatorClient>,
    wireguard: WireGuard,
    managed: Mutex<HashMap<String, DesiredPeer>>,
    managed_count: Arc<AtomicUsize>,
    last_lease_renewal: Mutex<Instant>,
}

impl PeerReconciler {
    pub fn new(coordinator: Arc<CoordinatorClient>, wireguard: WireGuard) -> Arc<Self> {
        Arc::new(Self {
            coordinator,
            wireguard,
            managed: Mutex::new(HashMap::new()),
            managed_count: Arc::new(AtomicUsize::new(0)),
            last_lease_renewal: Mutex::new(Instant::now()),
        })
    }

    pub fn spawn(self: &Arc<Self>) {
        let reconciler = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                if let Err(error) = reconciler.reconcile_once().await {
                    error!(error = %error, "failed to reconcile coordinator WireGuard peers");
                }
            }
        });
    }

    pub async fn reconcile_once(&self) -> Result<()> {
        remove_expired(&self.wireguard, &self.managed).await;
        self.sync_managed_count().await;
        self.renew_active_leases().await;
        let snapshot = self.coordinator.peer_snapshot().await?;
        if snapshot
            .peers
            .iter()
            .any(|peer| peer.lease_expires_at <= Utc::now())
        {
            warn!(
                revision = snapshot.revision,
                "coordinator snapshot contains an expired peer lease"
            );
            return Ok(());
        }
        let applied = apply_snapshot(&self.wireguard, &self.managed, &snapshot).await;
        self.sync_managed_count().await;
        applied?;
        self.coordinator
            .acknowledge_peers(snapshot.revision, self.managed.lock().await.len() as u64)
            .await?;
        Ok(())
    }

    pub fn managed_count_handle(&self) -> Arc<AtomicUsize> {
        self.managed_count.clone()
    }

    async fn sync_managed_count(&self) {
        self.managed_count
            .store(self.managed.lock().await.len(), Ordering::Relaxed);
    }

    async fn renew_active_leases(&self) {
        let mut last_renewal = self.last_lease_renewal.lock().await;
        if last_renewal.elapsed() < Duration::from_secs(30) {
            return;
        }
        *last_renewal = Instant::now();
        drop(last_renewal);
        let session_ids = self
            .managed
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for session_id in session_ids {
            if let Err(error) = self.coordinator.heartbeat(session_id.clone()).await {
                warn!(error = %error, session_id, "failed to renew coordinated session lease");
            }
        }
    }

    pub async fn cleanup(&self) {
        let peers = self
            .managed
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for peer in peers {
            if let Err(error) = self.wireguard.remove_peer(&peer.client_public_key).await {
                error!(error = %error, "failed to clean up coordinated WireGuard peer");
            }
        }
        self.managed.lock().await.clear();
        self.managed_count.store(0, Ordering::Relaxed);
    }
}

async fn apply_snapshot(
    wireguard: &WireGuard,
    managed: &Mutex<HashMap<String, DesiredPeer>>,
    snapshot: &PeerSnapshot,
) -> Result<()> {
    let desired: HashMap<_, _> = snapshot
        .peers
        .iter()
        .cloned()
        .map(|peer| (peer.session_id.clone(), peer))
        .collect();
    let removals = {
        let managed = managed.lock().await;
        managed
            .iter()
            .filter(|(session_id, peer)| {
                desired
                    .get(*session_id)
                    .is_none_or(|desired| !same_peer_configuration(peer, desired))
            })
            .map(|(session_id, peer)| (session_id.clone(), peer.clone()))
            .collect::<Vec<_>>()
    };
    for (session_id, peer) in removals {
        wireguard.remove_peer(&peer.client_public_key).await?;
        managed.lock().await.remove(&session_id);
    }

    for (session_id, peer) in desired {
        if managed
            .lock()
            .await
            .get(&session_id)
            .is_some_and(|current| same_peer_configuration(current, &peer))
        {
            managed.lock().await.insert(session_id, peer);
            continue;
        }
        wireguard
            .add_peer(&peer.client_public_key, &peer_allowed_ip(&peer.assigned_ip))
            .await?;
        managed.lock().await.insert(session_id, peer);
    }
    Ok(())
}

fn same_peer_configuration(left: &DesiredPeer, right: &DesiredPeer) -> bool {
    left.client_public_key == right.client_public_key && left.assigned_ip == right.assigned_ip
}

fn peer_allowed_ip(address: &str) -> String {
    if address.contains('/') {
        address.to_string()
    } else {
        format!("{address}/32")
    }
}

async fn remove_expired(wireguard: &WireGuard, managed: &Mutex<HashMap<String, DesiredPeer>>) {
    let now = Utc::now();
    let expired = {
        let managed = managed.lock().await;
        managed
            .iter()
            .filter(|(_, peer)| peer.lease_expires_at <= now)
            .map(|(session_id, peer)| (session_id.clone(), peer.client_public_key.clone()))
            .collect::<Vec<_>>()
    };
    for (session_id, public_key) in expired {
        if let Err(error) = wireguard.remove_peer(&public_key).await {
            error!(error = %error, session_id, "failed to remove peer after local lease expiry");
            continue;
        }
        managed.lock().await.remove(&session_id);
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration as ChronoDuration;

    use super::*;

    fn peer(session_id: &str, key: &str) -> DesiredPeer {
        DesiredPeer {
            session_id: session_id.into(),
            client_public_key: key.into(),
            assigned_ip: "10.8.0.2".into(),
            lease_expires_at: Utc::now() + ChronoDuration::seconds(90),
        }
    }

    #[tokio::test]
    async fn applies_only_the_coordinator_managed_snapshot() {
        let wireguard = WireGuard::new("wg".into(), "wg0".into(), true);
        let managed = Mutex::new(HashMap::new());
        let snapshot = PeerSnapshot {
            logical_node: "node-a".into(),
            generation_id: "green".into(),
            revision: 1,
            peers: vec![peer("session-a", "key-a")],
        };

        apply_snapshot(&wireguard, &managed, &snapshot)
            .await
            .unwrap();
        assert_eq!(managed.lock().await.len(), 1);

        apply_snapshot(
            &wireguard,
            &managed,
            &PeerSnapshot {
                peers: vec![peer("session-a", "key-b")],
                revision: 2,
                ..snapshot
            },
        )
        .await
        .unwrap();
        assert_eq!(managed.lock().await["session-a"].client_public_key, "key-b");
    }

    #[tokio::test]
    async fn restart_snapshot_rebuilds_state_and_expired_lease_removes_it_locally() {
        let wireguard = WireGuard::new("wg".into(), "wg0".into(), true);
        let managed = Mutex::new(HashMap::new());
        let mut expired = peer("session-a", "key-a");
        expired.lease_expires_at = Utc::now() - ChronoDuration::seconds(1);
        let snapshot = PeerSnapshot {
            logical_node: "node-a".into(),
            generation_id: "green".into(),
            revision: 9,
            peers: vec![expired],
        };

        apply_snapshot(&wireguard, &managed, &snapshot)
            .await
            .unwrap();
        assert_eq!(managed.lock().await.len(), 1);
        remove_expired(&wireguard, &managed).await;
        assert!(managed.lock().await.is_empty());
    }

    #[tokio::test]
    async fn failed_wireguard_command_is_not_recorded_or_acknowledgeable() {
        let wireguard = WireGuard::new("false".into(), "wg0".into(), false);
        let managed = Mutex::new(HashMap::new());
        let snapshot = PeerSnapshot {
            logical_node: "node-a".into(),
            generation_id: "green".into(),
            revision: 1,
            peers: vec![peer("session-a", "key-a")],
        };

        assert!(apply_snapshot(&wireguard, &managed, &snapshot)
            .await
            .is_err());
        assert!(managed.lock().await.is_empty());
    }
}
