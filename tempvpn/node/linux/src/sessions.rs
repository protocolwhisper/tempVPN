use std::{
    collections::{HashMap, HashSet},
    net::Ipv4Addr,
    sync::Arc,
};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    config::Config,
    error::{Error, Result},
    ip_allocator::IpAllocator,
    wireguard::WireGuard,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Active,
    Paused,
    Expired,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub session_id: String,
    pub node_url: String,
    pub client_public_key: Option<String>,
    pub assigned_ip: Option<String>,
    pub server_public_key: String,
    pub endpoint: String,
    pub expected_exit_ip: String,
    pub created_at: DateTime<Utc>,
    pub connected_at: Option<DateTime<Utc>>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub not_after: DateTime<Utc>,
    pub total_seconds: u64,
    pub remaining_seconds: u64,
    pub state: SessionState,
}

#[derive(Debug)]
struct SessionRecord {
    session: Session,
    ip: Option<Ipv4Addr>,
}

#[derive(Debug, Default)]
struct SessionStore {
    sessions: HashMap<String, SessionRecord>,
    allocated_ips: HashSet<Ipv4Addr>,
}

#[derive(Debug)]
pub struct Sessions {
    store: Mutex<SessionStore>,
    allocator: IpAllocator,
    wireguard: WireGuard,
    server_public_key: String,
    endpoint: String,
    expected_exit_ip: String,
    node_url: String,
    max_duration_seconds: u64,
    grace_period_seconds: u64,
    stale_timeout_seconds: u64,
}

impl Sessions {
    pub fn new(config: &Config) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            store: Mutex::new(SessionStore::default()),
            allocator: IpAllocator::new(&config.tunnel_cidr)?,
            wireguard: WireGuard::new(
                config.wg_command.clone(),
                config.wg_interface.clone(),
                config.mock_wg,
            ),
            server_public_key: config.server_public_key.clone(),
            endpoint: config.endpoint.clone(),
            expected_exit_ip: config.expected_exit_ip.clone(),
            node_url: config.public_api_url.trim_end_matches('/').to_string(),
            max_duration_seconds: config.max_duration_seconds,
            grace_period_seconds: config.grace_period_seconds,
            stale_timeout_seconds: config.stale_timeout_seconds,
        }))
    }

    pub async fn create(&self, duration_seconds: u64) -> Result<Session> {
        if duration_seconds == 0 || duration_seconds > self.max_duration_seconds {
            return Err(Error::InvalidRequest(format!(
                "duration_seconds must be between 1 and {}",
                self.max_duration_seconds
            )));
        }

        let now = Utc::now();
        let session_id = format!("sess_{}", Uuid::new_v4().simple());
        let session = Session {
            session_id: session_id.clone(),
            node_url: self.node_url.clone(),
            client_public_key: None,
            assigned_ip: None,
            server_public_key: self.server_public_key.clone(),
            endpoint: self.endpoint.clone(),
            expected_exit_ip: self.expected_exit_ip.clone(),
            created_at: now,
            connected_at: None,
            last_heartbeat_at: None,
            not_after: now + Duration::seconds(self.grace_period_seconds as i64),
            total_seconds: duration_seconds,
            remaining_seconds: duration_seconds,
            state: SessionState::Paused,
        };

        let mut store = self.store.lock().await;
        store.sessions.insert(
            session_id,
            SessionRecord {
                session: session.clone(),
                ip: None,
            },
        );
        info!(
            session_id = session.session_id,
            remaining_seconds = session.remaining_seconds,
            not_after = %session.not_after,
            "created usage-balance session"
        );
        Ok(session)
    }

    pub async fn connect(&self, session_id: &str, client_public_key: String) -> Result<Session> {
        if client_public_key.trim().is_empty() {
            return Err(Error::InvalidRequest(
                "client_public_key is required".to_string(),
            ));
        }

        let now = Utc::now();
        let (session, old_public_key, ip) = {
            let mut store = self.store.lock().await;
            let remaining_allocated = store.allocated_ips.clone();
            let mut newly_allocated_ip = None;
            let (session, old_public_key, ip) = {
                let record = store
                    .sessions
                    .get_mut(session_id)
                    .ok_or_else(|| Error::InvalidRequest("session not found".to_string()))?;

                refresh_usage(
                    &mut record.session,
                    now,
                    self.stale_timeout_seconds,
                    UsageRefreshMode::Now,
                );
                ensure_connectable(&record.session, now)?;

                let old_public_key = record.session.client_public_key.clone();
                let ip = match record.ip {
                    Some(ip) => ip,
                    None => {
                        let ip = self
                            .allocator
                            .allocate(&remaining_allocated)
                            .ok_or(Error::NoFreeTunnelIps)?;
                        newly_allocated_ip = Some(ip);
                        record.ip = Some(ip);
                        ip
                    }
                };

                record.session.client_public_key = Some(client_public_key.clone());
                record.session.assigned_ip = Some(self.allocator.peer_cidr(ip));
                record.session.connected_at = Some(now);
                record.session.last_heartbeat_at = Some(now);
                record.session.state = SessionState::Active;
                (record.session.clone(), old_public_key, ip)
            };
            if let Some(ip) = newly_allocated_ip {
                store.allocated_ips.insert(ip);
            }
            (session, old_public_key, ip)
        };

        if let Some(old_public_key) = old_public_key.filter(|key| key != &client_public_key) {
            if let Err(err) = self.wireguard.remove_peer(&old_public_key).await {
                warn!(session_id, error = %err, "failed to remove old peer before reconnect");
            }
        }

        if let Err(err) = self
            .wireguard
            .add_peer(
                &client_public_key,
                session.assigned_ip.as_deref().unwrap_or_default(),
            )
            .await
        {
            let mut store = self.store.lock().await;
            if let Some(record) = store.sessions.get_mut(session_id) {
                record.session.state = SessionState::Paused;
                record.session.connected_at = None;
                record.session.last_heartbeat_at = None;
                record.session.client_public_key = None;
                record.session.assigned_ip = None;
                record.ip = None;
            }
            store.allocated_ips.remove(&ip);
            return Err(err);
        }

        info!(
            session_id,
            remaining_seconds = session.remaining_seconds,
            "connected session"
        );
        Ok(session)
    }

    pub async fn heartbeat(&self, session_id: &str) -> Result<Session> {
        let mut store = self.store.lock().await;
        let record = store
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| Error::InvalidRequest("session not found".to_string()))?;
        let now = Utc::now();
        refresh_usage(
            &mut record.session,
            now,
            self.stale_timeout_seconds,
            UsageRefreshMode::Now,
        );
        if record.session.state == SessionState::Active {
            record.session.last_heartbeat_at = Some(now);
        }
        Ok(record.session.clone())
    }

    pub async fn pause(&self, session_id: &str) -> Result<Option<Session>> {
        let removed_public_key = {
            let mut store = self.store.lock().await;
            let Some(record) = store.sessions.get_mut(session_id) else {
                return Ok(None);
            };
            let now = Utc::now();
            refresh_usage(
                &mut record.session,
                now,
                self.stale_timeout_seconds,
                UsageRefreshMode::Now,
            );
            record.session.state = if record.session.remaining_seconds == 0 {
                SessionState::Expired
            } else {
                SessionState::Paused
            };
            record.session.connected_at = None;
            record.session.last_heartbeat_at = None;
            record.session.client_public_key.take()
        };

        if let Some(public_key) = removed_public_key {
            self.wireguard.remove_peer(&public_key).await?;
        }

        Ok(self.get(session_id).await)
    }

    /// Pause a metered stream while retaining its peer binding for reconnection.
    pub async fn pause_for_stream(&self, session_id: &str) -> Result<Option<Session>> {
        let removed_public_key = {
            let mut store = self.store.lock().await;
            let Some(record) = store.sessions.get_mut(session_id) else {
                return Ok(None);
            };
            let now = Utc::now();
            refresh_usage(
                &mut record.session,
                now,
                self.stale_timeout_seconds,
                UsageRefreshMode::Now,
            );
            record.session.state = if record.session.remaining_seconds == 0 {
                SessionState::Expired
            } else {
                SessionState::Paused
            };
            record.session.connected_at = None;
            record.session.last_heartbeat_at = None;
            record.session.client_public_key.clone()
        };

        if let Some(public_key) = removed_public_key {
            self.wireguard.remove_peer(&public_key).await?;
        }
        Ok(self.get(session_id).await)
    }

    /// Re-enable a paused metered stream using its previously bound peer key.
    pub async fn resume_for_stream(&self, session_id: &str) -> Result<Option<Session>> {
        let public_key = {
            let store = self.store.lock().await;
            let Some(record) = store.sessions.get(session_id) else {
                return Ok(None);
            };
            record.session.client_public_key.clone()
        };
        let Some(public_key) = public_key else {
            return Ok(None);
        };
        self.connect(session_id, public_key).await.map(Some)
    }

    pub async fn is_active(&self, session_id: &str) -> bool {
        self.get(session_id)
            .await
            .is_some_and(|session| session.state == SessionState::Active)
    }

    pub async fn disable_peer_by_public_key(&self, public_key: &str) -> Result<()> {
        self.wireguard.remove_peer(public_key).await?;
        let mut store = self.store.lock().await;
        for record in store.sessions.values_mut() {
            if record.session.client_public_key.as_deref() == Some(public_key) {
                record.session.state = SessionState::Paused;
                record.session.connected_at = None;
                record.session.last_heartbeat_at = None;
            }
        }
        Ok(())
    }

    pub async fn remove(&self, session_id: &str) -> Result<Option<Session>> {
        let removed = {
            let mut store = self.store.lock().await;
            let Some(record) = store.sessions.remove(session_id) else {
                return Ok(None);
            };
            if let Some(ip) = record.ip {
                store.allocated_ips.remove(&ip);
            }
            record.session
        };

        if let Some(public_key) = &removed.client_public_key {
            self.wireguard
                .remove_peer(public_key)
                .await
                .map_err(|err| {
                    error!(session_id, error = %err, "failed to remove WireGuard peer");
                    err
                })?;
        }
        info!(session_id, "removed session");
        Ok(Some(removed))
    }

    pub async fn get(&self, session_id: &str) -> Option<Session> {
        let mut store = self.store.lock().await;
        let record = store.sessions.get_mut(session_id)?;
        refresh_usage(
            &mut record.session,
            Utc::now(),
            self.stale_timeout_seconds,
            UsageRefreshMode::Now,
        );
        Some(record.session.clone())
    }

    pub async fn expire_sessions(&self) {
        let now = Utc::now();
        let expired = {
            let mut store = self.store.lock().await;
            let stale_timeout_seconds = self.stale_timeout_seconds;
            let mut expired_ids = Vec::new();
            let mut stale_paused = Vec::new();

            for (id, record) in store.sessions.iter_mut() {
                refresh_usage(
                    &mut record.session,
                    now,
                    stale_timeout_seconds,
                    UsageRefreshMode::StaleCutoff,
                );
                if record.session.state == SessionState::Active
                    && record
                        .session
                        .last_heartbeat_at
                        .map(|last| now - last > Duration::seconds(stale_timeout_seconds as i64))
                        .unwrap_or(false)
                {
                    record.session.state = SessionState::Paused;
                    record.session.connected_at = None;
                    record.session.last_heartbeat_at = None;
                    stale_paused.push(id.clone());
                }
                if record.session.remaining_seconds == 0 || record.session.not_after <= now {
                    record.session.state = SessionState::Expired;
                    expired_ids.push(id.clone());
                }
            }

            for id in &stale_paused {
                warn!(session_id = id, "pausing stale session");
            }

            expired_ids
                .into_iter()
                .filter_map(|id| {
                    let record = store.sessions.remove(&id)?;
                    if let Some(ip) = record.ip {
                        store.allocated_ips.remove(&ip);
                    }
                    Some(record.session)
                })
                .collect::<Vec<_>>()
        };

        for session in expired {
            warn!(session_id = session.session_id, "expiring session");
            if let Some(public_key) = session.client_public_key {
                if let Err(err) = self.wireguard.remove_peer(&public_key).await {
                    error!(
                        session_id = session.session_id,
                        error = %err,
                        "failed to remove expired peer"
                    );
                }
            }
        }
    }

    pub async fn cleanup_all(&self) {
        let active = {
            let mut store = self.store.lock().await;
            let sessions = store
                .sessions
                .drain()
                .map(|(_, record)| record.session)
                .collect::<Vec<_>>();
            store.allocated_ips.clear();
            sessions
        };

        for session in active {
            if let Some(public_key) = session.client_public_key {
                if let Err(err) = self.wireguard.remove_peer(&public_key).await {
                    error!(
                        session_id = session.session_id,
                        error = %err,
                        "failed to remove peer during shutdown"
                    );
                }
            }
        }
    }

    pub async fn active_count(&self) -> usize {
        self.store
            .lock()
            .await
            .sessions
            .values()
            .filter(|record| record.session.state == SessionState::Active)
            .count()
    }

    pub async fn available_slots(&self) -> usize {
        let store = self.store.lock().await;
        self.allocator.available_slots(store.allocated_ips.len())
    }
}

#[derive(Debug, Clone, Copy)]
enum UsageRefreshMode {
    Now,
    StaleCutoff,
}

fn refresh_usage(
    session: &mut Session,
    now: DateTime<Utc>,
    stale_timeout_seconds: u64,
    mode: UsageRefreshMode,
) {
    if session.state != SessionState::Active {
        if session.not_after <= now {
            session.state = SessionState::Expired;
            session.remaining_seconds = 0;
        }
        return;
    }

    let Some(connected_at) = session.connected_at else {
        return;
    };
    let end = match mode {
        UsageRefreshMode::Now => now,
        UsageRefreshMode::StaleCutoff => session
            .last_heartbeat_at
            .map(|last| last + Duration::seconds(stale_timeout_seconds as i64))
            .filter(|cutoff| *cutoff < now)
            .unwrap_or(now),
    };
    let elapsed = (end - connected_at).num_seconds().max(0) as u64;
    session.remaining_seconds = session.remaining_seconds.saturating_sub(elapsed);
    session.connected_at = Some(end);

    if session.remaining_seconds == 0 || session.not_after <= now {
        session.state = SessionState::Expired;
        session.remaining_seconds = 0;
    }
}

fn ensure_connectable(session: &Session, now: DateTime<Utc>) -> Result<()> {
    if session.not_after <= now {
        return Err(Error::InvalidRequest(
            "session grace deadline passed".to_string(),
        ));
    }
    if session.remaining_seconds == 0 || session.state == SessionState::Expired {
        return Err(Error::InvalidRequest(
            "session has no time remaining".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    fn test_config(max_duration_seconds: u64) -> Config {
        Config {
            bind_addr: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
            admin_token: "test-admin".to_string(),
            node_id: "test".to_string(),
            node_name: "Test Node".to_string(),
            node_region: "local".to_string(),
            node_country_code: Some("DE".to_string()),
            node_subdivision_code: None,
            node_city: Some("Frankfurt".to_string()),
            accepting_sessions: true,
            public_api_url: "http://127.0.0.1:8080".to_string(),
            expected_exit_ip: "127.0.0.1".to_string(),
            registry_mode: false,
            registry_url: None,
            registry_token: None,
            registry_refresh_seconds: 30,
            registry_lease_seconds: 90,
            wg_interface: "wg0".to_string(),
            wg_command: "wg".to_string(),
            server_public_key: "server-public-key".to_string(),
            endpoint: "127.0.0.1:51820".to_string(),
            tunnel_cidr: "10.8.0.0/24".to_string(),
            max_duration_seconds,
            grace_period_seconds: 604800,
            stale_timeout_seconds: 90,
            sweep_interval_seconds: 10,
            cleanup_on_shutdown: true,
            mock_wg: true,
            mpp_rpc_url: "http://localhost".to_string(),
            mpp_realm: "localhost:8080".to_string(),
            mpp_payment_currency: "currency".to_string(),
            mpp_payment_recipient: "recipient".to_string(),
            coordinator: None,
            streaming: crate::config::StreamingConfig {
                enabled: false,
                mode: crate::config::StreamingMode::Development,
                chain_id: 42_431,
                reserve: "0x4d50500000000000000000000000000000000000"
                    .parse()
                    .unwrap(),
                operator: "0x0000000000000000000000000000000000000001"
                    .parse()
                    .unwrap(),
                unit_amount: 1_000,
                billing_interval_seconds: 60,
                suggested_reserve: 10_000,
                min_voucher_delta: 500,
                grace_period_seconds: 30,
                close_signer: None,
                store: crate::config::ChannelStoreConfig::Memory,
            },
        }
    }

    #[tokio::test]
    async fn create_starts_paused_with_full_balance() {
        let sessions = Sessions::new(&test_config(3600)).unwrap();
        let session = sessions.create(1800).await.unwrap();

        assert_eq!(session.state, SessionState::Paused);
        assert_eq!(session.total_seconds, 1800);
        assert_eq!(session.remaining_seconds, 1800);
        assert!(session.assigned_ip.is_none());
        assert!(session.connected_at.is_none());
    }

    #[tokio::test]
    async fn connect_assigns_ip_and_marks_active() {
        let sessions = Sessions::new(&test_config(3600)).unwrap();
        let created = sessions.create(1800).await.unwrap();
        let connected = sessions
            .connect(&created.session_id, "client-public-key".to_string())
            .await
            .unwrap();

        assert_eq!(connected.state, SessionState::Active);
        assert_eq!(
            connected.client_public_key.as_deref(),
            Some("client-public-key")
        );
        assert_eq!(connected.assigned_ip.as_deref(), Some("10.8.0.2/32"));
        assert!(connected.connected_at.is_some());
        assert_eq!(sessions.available_slots().await, 252);
    }

    #[tokio::test]
    async fn pause_preserves_remaining_balance() {
        let sessions = Sessions::new(&test_config(3600)).unwrap();
        let created = sessions.create(1800).await.unwrap();
        let connected = sessions
            .connect(&created.session_id, "client-public-key".to_string())
            .await
            .unwrap();
        let paused = sessions
            .pause(&connected.session_id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(paused.state, SessionState::Paused);
        assert!(paused.remaining_seconds > 0);
        assert!(paused.client_public_key.is_none());
        assert!(paused.connected_at.is_none());
    }

    #[tokio::test]
    async fn expired_balance_cannot_reconnect() {
        let sessions = Sessions::new(&test_config(1)).unwrap();
        let created = sessions.create(1).await.unwrap();
        sessions
            .connect(&created.session_id, "client-public-key".to_string())
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        sessions.expire_sessions().await;

        let err = sessions
            .connect(&created.session_id, "client-public-key".to_string())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("session not found"));
    }
}
