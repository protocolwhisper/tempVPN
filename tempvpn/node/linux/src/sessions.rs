use std::{
    collections::{HashMap, HashSet},
    net::Ipv4Addr,
    sync::Arc,
};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    config::Config,
    error::{Error, Result},
    ip_allocator::IpAllocator,
    node_state::{AuditEvent, NodeStateStore, PersistedSession, SavePaymentResult},
    wireguard::WireGuard,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Active,
    Paused,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
struct SessionRecord {
    session: Session,
    ip: Option<Ipv4Addr>,
    peer_cleanup_pending: bool,
    pending_peer_public_key: Option<String>,
    durable: bool,
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
    node_id: String,
    node_state: NodeStateStore,
}

impl Sessions {
    pub fn new(config: &Config) -> Result<Arc<Self>> {
        let allocator = IpAllocator::new(&config.tunnel_cidr)?;
        let node_state = NodeStateStore::open(&config.node_state_store, &config.node_id)?;
        let mut store = SessionStore::default();
        for persisted in node_state.load_sessions()? {
            if let Some(ip) = persisted.ip {
                if !store.allocated_ips.insert(ip) {
                    return Err(Error::Store(format!(
                        "duplicate persisted tunnel address {ip}"
                    )));
                }
            }
            let session_id = persisted.session.session_id.clone();
            if store
                .sessions
                .insert(
                    session_id.clone(),
                    SessionRecord {
                        session: persisted.session,
                        ip: persisted.ip,
                        peer_cleanup_pending: persisted.pending_peer_public_key.is_some(),
                        pending_peer_public_key: persisted.pending_peer_public_key,
                        durable: true,
                    },
                )
                .is_some()
            {
                return Err(Error::Store(format!(
                    "duplicate persisted session {session_id}"
                )));
            }
        }
        Ok(Arc::new(Self {
            store: Mutex::new(store),
            allocator,
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
            node_id: config.node_id.clone(),
            node_state,
        }))
    }

    pub async fn create(&self, duration_seconds: u64) -> Result<Session> {
        self.create_internal(duration_seconds, self.node_state.is_durable(), None)
            .await
    }

    pub async fn create_ephemeral(&self, duration_seconds: u64) -> Result<Session> {
        self.create_internal(duration_seconds, false, None).await
    }

    pub async fn create_paid(
        &self,
        duration_seconds: u64,
        receipt_reference: &str,
        amount: &str,
        currency: &str,
    ) -> Result<Session> {
        let event = AuditEvent {
            event_key: format!("payment:fixed:{receipt_reference}"),
            event_type: "payment_accepted".into(),
            intent: Some("charge".into()),
            action: Some("create_session".into()),
            receipt_reference: Some(receipt_reference.to_owned()),
            amount: Some(amount.to_owned()),
            currency: Some(currency.to_owned()),
            duration_seconds: Some(duration_seconds),
            ..AuditEvent::default()
        };
        let session = self
            .create_internal(duration_seconds, true, Some(event))
            .await?;
        info!(
            event = "payment_accepted",
            node_id = %self.node_id,
            intent = "charge",
            action = "create_session",
            receipt_reference,
            session_id = %session.session_id,
            amount,
            currency,
            duration_seconds,
            "accepted fixed-session payment"
        );
        Ok(session)
    }

    async fn create_internal(
        &self,
        duration_seconds: u64,
        durable: bool,
        payment_event: Option<AuditEvent>,
    ) -> Result<Session> {
        if duration_seconds == 0
            || duration_seconds > self.max_duration_seconds
            || duration_seconds % 60 != 0
        {
            return Err(Error::InvalidRequest(format!(
                "duration_seconds must be a positive multiple of 60 no greater than {}",
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
        let record = SessionRecord {
            session: session.clone(),
            ip: None,
            peer_cleanup_pending: false,
            pending_peer_public_key: None,
            durable,
        };
        if durable {
            let persisted = persisted_session(&record);
            if let Some(mut event) = payment_event {
                event.session_id = Some(session.session_id.clone());
                event.remaining_seconds = Some(session.remaining_seconds);
                event.state = Some(session_state_name(session.state).into());
                match self
                    .node_state
                    .save_session_and_event(persisted, event)
                    .await?
                {
                    SavePaymentResult::Created => {}
                    SavePaymentResult::ExistingSession(existing_id) => {
                        return store
                            .sessions
                            .get(&existing_id)
                            .map(|record| record.session.clone())
                            .ok_or_else(|| {
                                Error::Store(format!(
                                    "durable payment refers to unavailable session {existing_id}"
                                ))
                            });
                    }
                }
            } else {
                self.node_state.save_session(persisted).await?;
            }
        }
        store.sessions.insert(session_id, record);
        info!(
            event = "session_created",
            node_id = %self.node_id,
            session_id = session.session_id,
            remaining_seconds = session.remaining_seconds,
            state = session_state_name(session.state),
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
            let original_record;
            let (session, old_public_key, ip) = {
                let record = store
                    .sessions
                    .get_mut(session_id)
                    .ok_or_else(|| Error::InvalidRequest("session not found".to_string()))?;
                original_record = record.clone();

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
                record.peer_cleanup_pending = false;
                record.pending_peer_public_key = None;
                record.session.assigned_ip = Some(self.allocator.peer_cidr(ip));
                record.session.connected_at = Some(now);
                record.session.last_heartbeat_at = Some(now);
                record.session.state = SessionState::Active;
                (record.session.clone(), old_public_key, ip)
            };
            if let Some(ip) = newly_allocated_ip {
                store.allocated_ips.insert(ip);
            }
            if let Some(record) = store
                .sessions
                .get(session_id)
                .filter(|record| record.durable)
            {
                if let Err(error) = self
                    .node_state
                    .save_session(persisted_session(record))
                    .await
                {
                    store
                        .sessions
                        .insert(session_id.to_owned(), original_record);
                    if let Some(ip) = newly_allocated_ip {
                        store.allocated_ips.remove(&ip);
                    }
                    return Err(error);
                }
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
                record.pending_peer_public_key = None;
                if record.durable {
                    self.node_state
                        .save_session(persisted_session(record))
                        .await?;
                }
            }
            store.allocated_ips.remove(&ip);
            return Err(err);
        }

        info!(
            event = "session_connected",
            node_id = %self.node_id,
            session_id,
            remaining_seconds = session.remaining_seconds,
            state = session_state_name(session.state),
            "connected session"
        );
        if let Some(record) = self
            .store
            .lock()
            .await
            .sessions
            .get(session_id)
            .filter(|record| record.durable)
            .cloned()
        {
            self.append_lifecycle_event("session_connected", &record.session)
                .await?;
        }
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
        let session = record.session.clone();
        if record.durable {
            self.node_state
                .save_session(persisted_session(record))
                .await?;
        }
        drop(store);
        info!(
            event = "session_heartbeat",
            node_id = %self.node_id,
            session_id,
            remaining_seconds = session.remaining_seconds,
            state = session_state_name(session.state),
            "accounted session heartbeat"
        );
        self.append_lifecycle_event("session_heartbeat", &session)
            .await?;
        Ok(session)
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
            let removed_public_key = record.session.client_public_key.take();
            if let Some(public_key) = &removed_public_key {
                record.pending_peer_public_key = Some(public_key.clone());
                record.peer_cleanup_pending = true;
            }
            if record.durable {
                self.node_state
                    .save_session(persisted_session(record))
                    .await?;
            }
            removed_public_key
        };

        if let Some(public_key) = removed_public_key {
            self.wireguard.remove_peer(&public_key).await?;
            let mut store = self.store.lock().await;
            if let Some(record) = store.sessions.get_mut(session_id) {
                if record.pending_peer_public_key.as_deref() == Some(&public_key) {
                    record.pending_peer_public_key = None;
                    record.peer_cleanup_pending = false;
                    if record.durable {
                        self.node_state
                            .save_session(persisted_session(record))
                            .await?;
                    }
                }
            }
        }
        let session = self.get(session_id).await;
        if let Some(session) = &session {
            info!(
                event = "session_paused",
                node_id = %self.node_id,
                session_id,
                remaining_seconds = session.remaining_seconds,
                state = session_state_name(session.state),
                "paused session"
            );
            self.append_lifecycle_event("session_paused", session)
                .await?;
        }
        Ok(session)
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
            record
        };

        if let Some(public_key) = removed
            .session
            .client_public_key
            .as_ref()
            .or(removed.pending_peer_public_key.as_ref())
        {
            self.wireguard
                .remove_peer(public_key)
                .await
                .map_err(|err| {
                    error!(session_id, error = %err, "failed to remove WireGuard peer");
                    err
                })?;
        }
        if removed.durable {
            self.node_state.delete_session(session_id).await?;
        }
        info!(
            event = "session_removed",
            node_id = %self.node_id,
            session_id,
            remaining_seconds = removed.session.remaining_seconds,
            state = session_state_name(removed.session.state),
            "removed session"
        );
        self.append_lifecycle_event("session_removed", &removed.session)
            .await?;
        Ok(Some(removed.session))
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
        if record.durable {
            if let Err(error) = self
                .node_state
                .save_session(persisted_session(record))
                .await
            {
                error!(
                    event = "node_state_write_failed",
                    node_id = %self.node_id,
                    session_id,
                    error = %error,
                    "failed to persist refreshed session status"
                );
            }
        }
        Some(record.session.clone())
    }

    pub async fn expire_sessions(&self) {
        let now = Utc::now();
        let (transitions, peer_cleanup) = {
            let mut store = self.store.lock().await;
            let stale_timeout_seconds = self.stale_timeout_seconds;
            let mut transitions = Vec::new();
            let mut snapshots = Vec::new();
            let mut released_ips = Vec::new();
            let mut ephemeral_terminal_ids = Vec::new();

            for (id, record) in store.sessions.iter_mut() {
                let previous_state = record.session.state;
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
                    if let Some(public_key) = record.session.client_public_key.take() {
                        record.pending_peer_public_key = Some(public_key);
                        record.peer_cleanup_pending = true;
                    }
                    transitions.push(("session_auto_paused", record.session.clone()));
                }
                if record.session.remaining_seconds == 0 || record.session.not_after <= now {
                    record.session.state = SessionState::Expired;
                    record.session.remaining_seconds = 0;
                    record.session.connected_at = None;
                    record.session.last_heartbeat_at = None;
                    if let Some(public_key) = record.session.client_public_key.take() {
                        record.pending_peer_public_key = Some(public_key);
                        record.peer_cleanup_pending = true;
                    }
                    if record.pending_peer_public_key.is_none() {
                        if let Some(ip) = record.ip.take() {
                            released_ips.push(ip);
                        }
                        record.session.assigned_ip = None;
                    }
                    if previous_state != SessionState::Expired {
                        transitions.push(("session_expired", record.session.clone()));
                    }
                }
                if record.durable {
                    snapshots.push(persisted_session(record));
                }
                if record.peer_cleanup_pending && record.pending_peer_public_key.is_none() {
                    record.peer_cleanup_pending = false;
                }
                if !record.durable
                    && record.session.state == SessionState::Expired
                    && record.pending_peer_public_key.is_none()
                {
                    ephemeral_terminal_ids.push(id.clone());
                }
            }
            for ip in released_ips {
                store.allocated_ips.remove(&ip);
            }
            for snapshot in snapshots {
                if let Err(error) = self.node_state.save_session(snapshot).await {
                    error!(
                        event = "node_state_write_failed",
                        node_id = %self.node_id,
                        error = %error,
                        "failed to persist session sweep"
                    );
                }
            }
            for id in ephemeral_terminal_ids {
                store.sessions.remove(&id);
            }
            let peer_cleanup = store
                .sessions
                .iter()
                .filter(|(_, record)| record.peer_cleanup_pending)
                .filter_map(|(id, record)| {
                    record
                        .pending_peer_public_key
                        .clone()
                        .map(|public_key| (id.clone(), public_key, record.durable))
                })
                .collect::<Vec<_>>();
            (transitions, peer_cleanup)
        };

        for (event_type, session) in transitions {
            warn!(
                event = event_type,
                node_id = %self.node_id,
                session_id = %session.session_id,
                remaining_seconds = session.remaining_seconds,
                state = session_state_name(session.state),
                "session lifecycle transition during sweep"
            );
            if let Err(error) = self.append_lifecycle_event(event_type, &session).await {
                error!(
                    event = "payment_audit_write_failed",
                    node_id = %self.node_id,
                    session_id = %session.session_id,
                    error = %error,
                    "failed to append lifecycle audit event"
                );
            }
        }

        for (session_id, public_key, durable) in peer_cleanup {
            match self.wireguard.remove_peer(&public_key).await {
                Ok(()) => {
                    let mut store = self.store.lock().await;
                    if let Some(record) = store.sessions.get_mut(&session_id) {
                        if record.pending_peer_public_key.as_deref() == Some(&public_key) {
                            record.pending_peer_public_key = None;
                            record.peer_cleanup_pending = false;
                            let expired = record.session.state == SessionState::Expired;
                            let released_ip = expired.then(|| record.ip.take()).flatten();
                            if expired {
                                record.session.assigned_ip = None;
                            }
                            if durable {
                                if let Err(error) = self
                                    .node_state
                                    .save_session(persisted_session(record))
                                    .await
                                {
                                    error!(
                                        event = "node_state_write_failed",
                                        node_id = %self.node_id,
                                        session_id,
                                        error = %error,
                                        "failed to persist completed peer cleanup"
                                    );
                                }
                            }
                            if let Some(ip) = released_ip {
                                store.allocated_ips.remove(&ip);
                            }
                        }
                    }
                    if !durable
                        && store
                            .sessions
                            .get(&session_id)
                            .is_some_and(|record| record.session.state == SessionState::Expired)
                    {
                        store.sessions.remove(&session_id);
                    }
                }
                Err(err) => {
                    error!(
                        session_id,
                        error = %err,
                        "failed to remove stale paused peer; will retry"
                    );
                }
            }
        }
    }

    pub async fn cleanup_all(&self) {
        let cleanup = {
            let mut store = self.store.lock().await;
            let now = Utc::now();
            let mut cleanup = Vec::new();
            let mut snapshots = Vec::new();
            for (session_id, record) in store.sessions.iter_mut() {
                refresh_usage(
                    &mut record.session,
                    now,
                    self.stale_timeout_seconds,
                    UsageRefreshMode::Now,
                );
                if record.session.state == SessionState::Active {
                    record.session.state = if record.session.remaining_seconds == 0 {
                        SessionState::Expired
                    } else {
                        SessionState::Paused
                    };
                }
                record.session.connected_at = None;
                record.session.last_heartbeat_at = None;
                if let Some(public_key) = record.session.client_public_key.take() {
                    record.pending_peer_public_key = Some(public_key);
                    record.peer_cleanup_pending = true;
                }
                if let Some(public_key) = record.pending_peer_public_key.clone() {
                    cleanup.push((session_id.clone(), public_key, record.durable));
                }
                if record.durable {
                    snapshots.push(persisted_session(record));
                }
            }
            for snapshot in snapshots {
                if let Err(error) = self.node_state.save_session(snapshot).await {
                    error!(
                        event = "node_state_write_failed",
                        node_id = %self.node_id,
                        error = %error,
                        "failed to persist session during shutdown"
                    );
                }
            }
            cleanup
        };

        for (session_id, public_key, durable) in cleanup {
            match self.wireguard.remove_peer(&public_key).await {
                Ok(()) => {
                    let mut store = self.store.lock().await;
                    if durable {
                        if let Some(record) = store.sessions.get_mut(&session_id) {
                            if record.pending_peer_public_key.as_deref() == Some(&public_key) {
                                record.pending_peer_public_key = None;
                                record.peer_cleanup_pending = false;
                                if let Err(error) = self
                                    .node_state
                                    .save_session(persisted_session(record))
                                    .await
                                {
                                    error!(
                                        event = "node_state_write_failed",
                                        node_id = %self.node_id,
                                        session_id,
                                        error = %error,
                                        "failed to persist shutdown peer cleanup"
                                    );
                                }
                            }
                        }
                    } else if let Some(record) = store.sessions.remove(&session_id) {
                        if let Some(ip) = record.ip {
                            store.allocated_ips.remove(&ip);
                        }
                    }
                }
                Err(err) => {
                    error!(
                        event = "peer_cleanup_failed",
                        node_id = %self.node_id,
                        session_id,
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

    pub async fn reconcile_startup(&self) -> Result<()> {
        let recovered = {
            let mut store = self.store.lock().await;
            let now = Utc::now();
            let mut recovered = Vec::new();
            let mut released_ips = Vec::new();
            for record in store.sessions.values_mut().filter(|record| record.durable) {
                let previous_state = record.session.state;
                if record.session.not_after <= now || record.session.remaining_seconds == 0 {
                    record.session.state = SessionState::Expired;
                    record.session.remaining_seconds = 0;
                    if let Some(ip) = record.ip.take() {
                        released_ips.push(ip);
                    }
                    record.session.assigned_ip = None;
                } else if record.session.state == SessionState::Active {
                    record.session.state = SessionState::Paused;
                }
                if let Some(public_key) = record.session.client_public_key.take() {
                    record.pending_peer_public_key = Some(public_key);
                    record.peer_cleanup_pending = true;
                }
                record.session.connected_at = None;
                record.session.last_heartbeat_at = None;
                recovered.push((previous_state, record.clone()));
            }
            for ip in released_ips {
                store.allocated_ips.remove(&ip);
            }
            recovered
        };

        for (previous_state, mut record) in recovered {
            if let Some(public_key) = record.pending_peer_public_key.clone() {
                self.wireguard
                    .remove_peer(&public_key)
                    .await
                    .map_err(|error| {
                        Error::Store(format!(
                            "cannot safely recover session {}: {error}",
                            record.session.session_id
                        ))
                    })?;
                record.pending_peer_public_key = None;
                record.peer_cleanup_pending = false;
            }
            {
                let mut store = self.store.lock().await;
                let live = store
                    .sessions
                    .get_mut(&record.session.session_id)
                    .ok_or_else(|| Error::Store("recovered session disappeared".into()))?;
                *live = record.clone();
                self.node_state
                    .save_session(persisted_session(live))
                    .await?;
            }
            info!(
                event = "session_recovered",
                node_id = %self.node_id,
                session_id = %record.session.session_id,
                previous_state = session_state_name(previous_state),
                state = session_state_name(record.session.state),
                remaining_seconds = record.session.remaining_seconds,
                "recovered durable session after startup"
            );
            self.append_lifecycle_event("session_recovered", &record.session)
                .await?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_stream_payment(
        &self,
        receipt_reference: &str,
        action: &str,
        channel_id: &str,
        session_id: Option<&str>,
        amount: Option<&str>,
        currency: &str,
        duration_seconds: Option<u64>,
    ) -> Result<()> {
        let event = AuditEvent {
            event_key: format!(
                "payment:session:{receipt_reference}:{action}:{channel_id}:{}",
                amount.unwrap_or("")
            ),
            event_type: "payment_accepted".into(),
            intent: Some("session".into()),
            action: Some(action.into()),
            receipt_reference: Some(receipt_reference.into()),
            session_id: session_id.map(str::to_owned),
            channel_id: Some(channel_id.into()),
            amount: amount.map(str::to_owned),
            currency: Some(currency.into()),
            duration_seconds,
            ..AuditEvent::default()
        };
        let inserted = self.node_state.append_event(event).await?;
        if self.node_state.is_durable() && !inserted {
            return Ok(());
        }
        info!(
            event = "payment_accepted",
            node_id = %self.node_id,
            intent = "session",
            action,
            receipt_reference,
            channel_id,
            session_id = session_id.unwrap_or(""),
            amount = amount.unwrap_or(""),
            currency,
            duration_seconds = duration_seconds.unwrap_or_default(),
            "accepted streaming payment credential"
        );
        Ok(())
    }

    pub async fn record_coordinated_fixed_payment(
        &self,
        receipt_reference: &str,
        session: &Session,
        amount: &str,
        currency: &str,
    ) -> Result<()> {
        let inserted = self
            .node_state
            .append_event(AuditEvent {
                event_key: format!("payment:fixed:{receipt_reference}"),
                event_type: "payment_accepted".into(),
                intent: Some("charge".into()),
                action: Some("create_session".into()),
                receipt_reference: Some(receipt_reference.into()),
                session_id: Some(session.session_id.clone()),
                amount: Some(amount.into()),
                currency: Some(currency.into()),
                duration_seconds: Some(session.total_seconds),
                remaining_seconds: Some(session.remaining_seconds),
                state: Some(session_state_name(session.state).into()),
                ..AuditEvent::default()
            })
            .await?;
        if self.node_state.is_durable() && !inserted {
            return Ok(());
        }
        info!(
            event = "payment_accepted",
            node_id = %self.node_id,
            intent = "charge",
            action = "create_session",
            receipt_reference,
            session_id = %session.session_id,
            amount,
            currency,
            duration_seconds = session.total_seconds,
            "accepted coordinator-backed fixed-session payment"
        );
        Ok(())
    }

    async fn append_lifecycle_event(&self, event_type: &str, session: &Session) -> Result<()> {
        if !self.node_state.is_durable() {
            return Ok(());
        }
        self.node_state
            .append_event(AuditEvent {
                event_key: format!(
                    "lifecycle:{}:{}:{}",
                    session.session_id,
                    event_type,
                    Uuid::new_v4().simple()
                ),
                event_type: event_type.into(),
                intent: Some("session_lifecycle".into()),
                session_id: Some(session.session_id.clone()),
                duration_seconds: Some(session.total_seconds),
                remaining_seconds: Some(session.remaining_seconds),
                state: Some(session_state_name(session.state).into()),
                ..AuditEvent::default()
            })
            .await?;
        Ok(())
    }
}

fn persisted_session(record: &SessionRecord) -> PersistedSession {
    PersistedSession {
        session: record.session.clone(),
        ip: record.ip,
        pending_peer_public_key: record.pending_peer_public_key.clone(),
    }
}

fn session_state_name(state: SessionState) -> &'static str {
    match state {
        SessionState::Active => "active",
        SessionState::Paused => "paused",
        SessionState::Expired => "expired",
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
    use tempfile::tempdir;

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
            public_fixed_sessions_enabled: true,
            fixed_session_registry_url: "https://registry.tempvpn.xyz".into(),
            control_plane_token: None,
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
            node_state_store: crate::config::NodeStateStoreConfig::Memory,
            audit_log_path: None,
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
    async fn stale_auto_pause_removes_wireguard_peer() {
        let mut config = test_config(3600);
        config.stale_timeout_seconds = 0;
        let sessions = Sessions::new(&config).unwrap();
        let created = sessions.create(1800).await.unwrap();
        sessions
            .connect(&created.session_id, "client-public-key".to_string())
            .await
            .unwrap();
        assert!(sessions.wireguard.mock_has_peer("client-public-key").await);

        sessions.expire_sessions().await;

        let paused = sessions.get(&created.session_id).await.unwrap();
        assert_eq!(paused.state, SessionState::Paused);
        assert!(!sessions.wireguard.mock_has_peer("client-public-key").await);
    }

    #[tokio::test]
    async fn stale_auto_pause_retries_failed_wireguard_removal() {
        let mut config = test_config(3600);
        config.stale_timeout_seconds = 0;
        let sessions = Sessions::new(&config).unwrap();
        let created = sessions.create(1800).await.unwrap();
        sessions
            .connect(&created.session_id, "client-public-key".to_string())
            .await
            .unwrap();
        sessions.wireguard.mock_fail_next_removals(1);

        sessions.expire_sessions().await;
        assert!(sessions.wireguard.mock_has_peer("client-public-key").await);

        sessions.expire_sessions().await;
        assert!(!sessions.wireguard.mock_has_peer("client-public-key").await);
    }

    #[tokio::test]
    async fn expired_balance_cannot_reconnect() {
        let sessions = Sessions::new(&test_config(60)).unwrap();
        let created = sessions.create(60).await.unwrap();
        sessions
            .connect(&created.session_id, "client-public-key".to_string())
            .await
            .unwrap();

        {
            let mut store = sessions.store.lock().await;
            let record = store.sessions.get_mut(&created.session_id).unwrap();
            record.session.remaining_seconds = 0;
        }
        sessions.expire_sessions().await;

        let err = sessions
            .connect(&created.session_id, "client-public-key".to_string())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("session not found"));
    }

    #[tokio::test]
    async fn session_store_rejects_partial_minutes() {
        let sessions = Sessions::new(&test_config(3600)).unwrap();

        for duration_seconds in [0, 1, 59, 61] {
            let error = sessions.create(duration_seconds).await.unwrap_err();
            assert!(error.to_string().contains("positive multiple of 60"));
        }
    }

    #[tokio::test]
    async fn paid_session_and_receipt_survive_restart_and_recover_paused() {
        let directory = tempdir().unwrap();
        let mut config = test_config(3600);
        config.node_state_store =
            crate::config::NodeStateStoreConfig::Sqlite(directory.path().join("node-state.sqlite"));

        let sessions = Sessions::new(&config).unwrap();
        let created = sessions
            .create_paid(1800, "0xpaid", "0.01", "0xcurrency")
            .await
            .unwrap();
        sessions
            .connect(&created.session_id, "client-public-key".into())
            .await
            .unwrap();
        assert_eq!(
            sessions
                .node_state
                .audit_event("payment:fixed:0xpaid")
                .unwrap()
                .unwrap()
                .session_id
                .as_deref(),
            Some(created.session_id.as_str())
        );
        drop(sessions);

        let recovered = Sessions::new(&config).unwrap();
        recovered.reconcile_startup().await.unwrap();
        let restored = recovered.get(&created.session_id).await.unwrap();
        assert_eq!(restored.state, SessionState::Paused);
        assert_eq!(restored.total_seconds, 1800);
        assert!(restored.remaining_seconds > 0);
        assert!(restored.client_public_key.is_none());
        assert_eq!(restored.assigned_ip.as_deref(), Some("10.8.0.2/32"));
    }

    #[tokio::test]
    async fn duplicate_fixed_receipt_returns_original_session() {
        let directory = tempdir().unwrap();
        let mut config = test_config(3600);
        config.node_state_store =
            crate::config::NodeStateStoreConfig::Sqlite(directory.path().join("node-state.sqlite"));
        let sessions = Sessions::new(&config).unwrap();

        let first = sessions
            .create_paid(60, "0xduplicate", "0.01", "0xcurrency")
            .await
            .unwrap();
        let duplicate = sessions
            .create_paid(60, "0xduplicate", "0.01", "0xcurrency")
            .await
            .unwrap();
        assert_eq!(duplicate.session_id, first.session_id);
        assert_eq!(sessions.store.lock().await.sessions.len(), 1);
    }

    #[tokio::test]
    async fn expired_paid_terminal_record_survives_restart() {
        let directory = tempdir().unwrap();
        let mut config = test_config(3600);
        config.node_state_store =
            crate::config::NodeStateStoreConfig::Sqlite(directory.path().join("node-state.sqlite"));
        let sessions = Sessions::new(&config).unwrap();
        let created = sessions
            .create_paid(60, "0xexpired", "0.01", "0xcurrency")
            .await
            .unwrap();
        {
            let mut store = sessions.store.lock().await;
            let record = store.sessions.get_mut(&created.session_id).unwrap();
            record.session.remaining_seconds = 0;
        }
        sessions.expire_sessions().await;
        drop(sessions);

        let recovered = Sessions::new(&config).unwrap();
        recovered.reconcile_startup().await.unwrap();
        let restored = recovered.get(&created.session_id).await.unwrap();
        assert_eq!(restored.state, SessionState::Expired);
        assert_eq!(restored.remaining_seconds, 0);
    }
}
