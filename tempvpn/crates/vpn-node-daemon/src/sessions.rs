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

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub session_id: String,
    pub client_public_key: String,
    pub assigned_ip: String,
    pub server_public_key: String,
    pub endpoint: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug)]
struct SessionRecord {
    session: Session,
    ip: Ipv4Addr,
}

#[derive(Debug, Default)]
struct SessionState {
    sessions: HashMap<String, SessionRecord>,
    allocated_ips: HashSet<Ipv4Addr>,
}

#[derive(Debug)]
pub struct Sessions {
    state: Mutex<SessionState>,
    allocator: IpAllocator,
    wireguard: WireGuard,
    server_public_key: String,
    endpoint: String,
    max_duration_seconds: u64,
}

impl Sessions {
    pub fn new(config: &Config) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            state: Mutex::new(SessionState::default()),
            allocator: IpAllocator::new(&config.tunnel_cidr)?,
            wireguard: WireGuard::new(
                config.wg_command.clone(),
                config.wg_interface.clone(),
                config.mock_wg,
            ),
            server_public_key: config.server_public_key.clone(),
            endpoint: config.endpoint.clone(),
            max_duration_seconds: config.max_duration_seconds,
        }))
    }

    pub async fn create(
        &self,
        client_public_key: String,
        duration_seconds: u64,
    ) -> Result<Session> {
        if client_public_key.trim().is_empty() {
            return Err(Error::InvalidRequest(
                "client_public_key is required".to_string(),
            ));
        }
        if duration_seconds == 0 || duration_seconds > self.max_duration_seconds {
            return Err(Error::InvalidRequest(format!(
                "duration_seconds must be between 1 and {}",
                self.max_duration_seconds
            )));
        }

        let now = Utc::now();
        let session_id = format!("sess_{}", Uuid::new_v4().simple());
        let expires_at = now + Duration::seconds(duration_seconds as i64);

        let (session, ip) = {
            let mut state = self.state.lock().await;
            let ip = self
                .allocator
                .allocate(&state.allocated_ips)
                .ok_or(Error::NoFreeTunnelIps)?;
            state.allocated_ips.insert(ip);
            let assigned_ip = self.allocator.peer_cidr(ip);
            let session = Session {
                session_id: session_id.clone(),
                client_public_key: client_public_key.clone(),
                assigned_ip,
                server_public_key: self.server_public_key.clone(),
                endpoint: self.endpoint.clone(),
                created_at: now,
                expires_at,
            };
            state.sessions.insert(
                session_id.clone(),
                SessionRecord {
                    session: session.clone(),
                    ip,
                },
            );
            (session, ip)
        };

        if let Err(err) = self
            .wireguard
            .add_peer(&client_public_key, &session.assigned_ip)
            .await
        {
            let mut state = self.state.lock().await;
            state.sessions.remove(&session_id);
            state.allocated_ips.remove(&ip);
            return Err(err);
        }

        info!(
            session_id = session.session_id,
            assigned_ip = session.assigned_ip,
            expires_at = %session.expires_at,
            "created session"
        );
        Ok(session)
    }

    pub async fn get(&self, session_id: &str) -> Option<Session> {
        let state = self.state.lock().await;
        state
            .sessions
            .get(session_id)
            .map(|record| record.session.clone())
    }

    pub async fn remove(&self, session_id: &str) -> Result<Option<Session>> {
        let removed = {
            let mut state = self.state.lock().await;
            let Some(record) = state.sessions.remove(session_id) else {
                return Ok(None);
            };
            state.allocated_ips.remove(&record.ip);
            record.session
        };

        self.wireguard
            .remove_peer(&removed.client_public_key)
            .await
            .map_err(|err| {
                error!(session_id, error = %err, "failed to remove WireGuard peer");
                err
            })?;
        info!(session_id, "removed session");
        Ok(Some(removed))
    }

    pub async fn expire_sessions(&self) {
        let now = Utc::now();
        let expired = {
            let mut state = self.state.lock().await;
            let expired_ids = state
                .sessions
                .iter()
                .filter_map(|(id, record)| (record.session.expires_at <= now).then(|| id.clone()))
                .collect::<Vec<_>>();

            expired_ids
                .into_iter()
                .filter_map(|id| {
                    let record = state.sessions.remove(&id)?;
                    state.allocated_ips.remove(&record.ip);
                    Some(record.session)
                })
                .collect::<Vec<_>>()
        };

        for session in expired {
            warn!(
                session_id = session.session_id,
                assigned_ip = session.assigned_ip,
                "expiring session"
            );
            if let Err(err) = self.wireguard.remove_peer(&session.client_public_key).await {
                error!(
                    session_id = session.session_id,
                    error = %err,
                    "failed to remove expired peer"
                );
            }
        }
    }

    pub async fn cleanup_all(&self) {
        let active = {
            let mut state = self.state.lock().await;
            let sessions = state
                .sessions
                .drain()
                .map(|(_, record)| record.session)
                .collect::<Vec<_>>();
            state.allocated_ips.clear();
            sessions
        };

        for session in active {
            if let Err(err) = self.wireguard.remove_peer(&session.client_public_key).await {
                error!(
                    session_id = session.session_id,
                    error = %err,
                    "failed to remove peer during shutdown"
                );
            }
        }
    }

    pub async fn active_count(&self) -> usize {
        self.state.lock().await.sessions.len()
    }
}
