use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use mpp::protocol::{
    methods::tempo::{
        session::ChannelDescriptor,
        session_method::{ChannelState, ChannelStore},
    },
    traits::VerificationError,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
    config::ChannelStoreConfig,
    error::{Error, Result},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamLease {
    pub owner_id: String,
    pub logical_session_id: String,
    pub client_public_key: String,
    pub expires_at_unix: i64,
}

impl StreamLease {
    pub fn is_expired_at(&self, now_unix: i64) -> bool {
        self.expires_at_unix <= now_unix
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChannel {
    pub accounting: ChannelState,
    pub descriptor: Option<ChannelDescriptor>,
    pub lease: Option<StreamLease>,
}

enum Backend {
    Memory(StdMutex<HashMap<String, StoredChannel>>),
    Sqlite(PathBuf),
}

/// Durable protocol state plus the local wake-up mechanism used by metered streams.
///
/// SQLite operations open a short-lived connection and use `BEGIN IMMEDIATE`, which
/// serializes the read/compare/write callback across every process sharing the file.
pub struct SessionStore {
    backend: Backend,
    updates: StdMutex<HashMap<String, watch::Sender<u64>>>,
}

impl SessionStore {
    pub async fn open(config: &ChannelStoreConfig) -> Result<Arc<Self>> {
        let backend = match config {
            ChannelStoreConfig::Memory => Backend::Memory(StdMutex::new(HashMap::new())),
            ChannelStoreConfig::Sqlite(path) => {
                initialize_sqlite(path).await?;
                Backend::Sqlite(path.clone())
            }
        };
        Ok(Arc::new(Self {
            backend,
            updates: StdMutex::new(HashMap::new()),
        }))
    }

    pub fn is_durable(&self) -> bool {
        matches!(self.backend, Backend::Sqlite(_))
    }

    /// Return leases whose heartbeat deadline has passed. Live leases are left
    /// untouched so starting another serving process cannot evict their streams.
    pub async fn reconcile_startup_leases(
        &self,
        now_unix: i64,
    ) -> std::result::Result<Vec<(String, StreamLease)>, VerificationError> {
        let rows = match &self.backend {
            Backend::Memory(rows) => rows
                .lock()
                .map_err(|_| store_error("memory store mutex poisoned"))?
                .iter()
                .map(|(id, row)| (id.clone(), row.clone()))
                .collect::<Vec<_>>(),
            Backend::Sqlite(path) => {
                let path = path.clone();
                tokio::task::spawn_blocking(move || sqlite_list(&path))
                    .await
                    .map_err(|err| store_error(format!("SQLite list task failed: {err}")))??
            }
        };
        let mut leases = Vec::new();
        for (channel_id, row) in rows {
            if let Some(lease) = row.lease {
                if lease.is_expired_at(now_unix) {
                    leases.push((channel_id, lease));
                }
            }
        }
        Ok(leases)
    }

    pub async fn get_stored(
        &self,
        channel_id: &str,
    ) -> std::result::Result<Option<StoredChannel>, VerificationError> {
        match &self.backend {
            Backend::Memory(rows) => Ok(rows
                .lock()
                .map_err(|_| store_error("memory store mutex poisoned"))?
                .get(channel_id)
                .cloned()),
            Backend::Sqlite(path) => {
                let path = path.clone();
                let channel_id = channel_id.to_owned();
                tokio::task::spawn_blocking(move || sqlite_read(&path, &channel_id))
                    .await
                    .map_err(|err| store_error(format!("SQLite read task failed: {err}")))?
            }
        }
    }

    /// Atomically installs a fully verified v2 channel snapshot.
    pub async fn upsert_verified(
        &self,
        state: ChannelState,
        descriptor: ChannelDescriptor,
    ) -> std::result::Result<StoredChannel, VerificationError> {
        let channel_id = state.channel_id.clone();
        let descriptor_for_update = descriptor.clone();
        let next = self
            .update_stored(&channel_id, move |current| {
                if let Some(mut current) = current {
                    if let Some(existing) = &current.descriptor {
                        if existing != &descriptor_for_update {
                            return Err(VerificationError::credential_mismatch(
                                "channel descriptor does not match durable state",
                            ));
                        }
                    }
                    if state.highest_voucher_amount < current.accounting.highest_voucher_amount {
                        return Err(VerificationError::new(
                            "verified snapshot would decrease accepted voucher amount",
                        ));
                    }
                    if state.spent < current.accounting.spent {
                        return Err(VerificationError::new(
                            "verified snapshot would decrease durable spend",
                        ));
                    }
                    current.accounting = state;
                    current.descriptor = Some(descriptor_for_update);
                    Ok(Some(current))
                } else {
                    Ok(Some(StoredChannel {
                        accounting: state,
                        descriptor: Some(descriptor_for_update),
                        lease: None,
                    }))
                }
            })
            .await?
            .ok_or_else(|| store_error("verified channel disappeared during update"))?;
        self.notify(&channel_id);
        Ok(next)
    }

    /// Acquire or refresh a stream owner. Another live owner fails closed.
    pub async fn acquire_lease(
        &self,
        channel_id: &str,
        lease: StreamLease,
        now_unix: i64,
    ) -> std::result::Result<StoredChannel, VerificationError> {
        let owner = lease.owner_id.clone();
        let next = self
            .update_stored(channel_id, move |current| {
                let mut current = current.ok_or_else(|| {
                    VerificationError::channel_not_found("channel not found for stream lease")
                })?;
                if current.accounting.finalized || current.accounting.closing {
                    return Err(VerificationError::channel_closed(
                        "cannot lease a finalized channel",
                    ));
                }
                if let Some(existing) = &current.lease {
                    if existing.owner_id != owner && !existing.is_expired_at(now_unix) {
                        return Err(VerificationError::new(
                            "payment channel is already owned by another active stream",
                        ));
                    }
                }
                current.lease = Some(lease);
                Ok(Some(current))
            })
            .await?
            .ok_or_else(|| store_error("leased channel disappeared during update"))?;
        self.notify(channel_id);
        Ok(next)
    }

    pub async fn release_lease(
        &self,
        channel_id: &str,
        owner_id: &str,
    ) -> std::result::Result<(), VerificationError> {
        let owner_id = owner_id.to_owned();
        self.update_stored(channel_id, move |current| {
            let Some(mut current) = current else {
                return Ok(None);
            };
            if current
                .lease
                .as_ref()
                .is_some_and(|lease| lease.owner_id == owner_id)
            {
                current.lease = None;
            }
            Ok(Some(current))
        })
        .await?;
        self.notify(channel_id);
        Ok(())
    }

    /// Mark the current owner's lease immediately reclaimable while retaining
    /// the logical session identity for grace-period reconnects.
    pub async fn expire_lease(
        &self,
        channel_id: &str,
        owner_id: &str,
        now_unix: i64,
    ) -> std::result::Result<(), VerificationError> {
        let owner_id = owner_id.to_owned();
        self.update_stored(channel_id, move |current| {
            let Some(mut current) = current else {
                return Ok(None);
            };
            if let Some(lease) = current.lease.as_mut() {
                if lease.owner_id == owner_id {
                    lease.expires_at_unix = now_unix;
                }
            }
            Ok(Some(current))
        })
        .await?;
        self.notify(channel_id);
        Ok(())
    }

    /// Atomically fence an expired lease for cleanup. The temporary cleanup
    /// owner prevents a reconnect from racing peer removal.
    pub async fn claim_expired_lease(
        &self,
        channel_id: &str,
        expected_owner: &str,
        cleanup_owner: &str,
        now_unix: i64,
        hold_until_unix: i64,
    ) -> std::result::Result<bool, VerificationError> {
        let expected_owner = expected_owner.to_owned();
        let cleanup_owner = cleanup_owner.to_owned();
        let cleanup_owner_for_update = cleanup_owner.clone();
        let updated = self
            .update_stored(channel_id, move |current| {
                let Some(mut current) = current else {
                    return Ok(None);
                };
                if let Some(lease) = current.lease.as_mut() {
                    if lease.owner_id == expected_owner && lease.is_expired_at(now_unix) {
                        lease.owner_id = cleanup_owner_for_update;
                        lease.expires_at_unix = hold_until_unix;
                    }
                }
                Ok(Some(current))
            })
            .await?;
        self.notify(channel_id);
        Ok(updated
            .and_then(|row| row.lease)
            .is_some_and(|lease| lease.owner_id == cleanup_owner))
    }

    async fn update_stored(
        &self,
        channel_id: &str,
        updater: impl FnOnce(
                Option<StoredChannel>,
            ) -> std::result::Result<Option<StoredChannel>, VerificationError>
            + Send
            + 'static,
    ) -> std::result::Result<Option<StoredChannel>, VerificationError> {
        match &self.backend {
            Backend::Memory(rows) => {
                let mut rows = rows
                    .lock()
                    .map_err(|_| store_error("memory store mutex poisoned"))?;
                let next = updater(rows.get(channel_id).cloned())?;
                match &next {
                    Some(row) => {
                        rows.insert(channel_id.to_owned(), row.clone());
                    }
                    None => {
                        rows.remove(channel_id);
                    }
                }
                Ok(next)
            }
            Backend::Sqlite(path) => {
                let path = path.clone();
                let channel_id = channel_id.to_owned();
                tokio::task::spawn_blocking(move || {
                    sqlite_update(&path, &channel_id, Box::new(updater))
                })
                .await
                .map_err(|err| store_error(format!("SQLite update task failed: {err}")))?
            }
        }
    }

    fn sender(&self, channel_id: &str) -> watch::Sender<u64> {
        let mut updates = self
            .updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        updates
            .entry(channel_id.to_owned())
            .or_insert_with(|| watch::channel(0).0)
            .clone()
    }

    fn notify(&self, channel_id: &str) {
        let sender = self.sender(channel_id);
        let next = {
            let current = sender.borrow();
            current.wrapping_add(1)
        };
        sender.send_replace(next);
    }
}

impl ChannelStore for SessionStore {
    fn get_channel(
        &self,
        channel_id: &str,
    ) -> Pin<
        Box<
            dyn Future<Output = std::result::Result<Option<ChannelState>, VerificationError>>
                + Send
                + '_,
        >,
    > {
        let channel_id = channel_id.to_owned();
        Box::pin(async move {
            Ok(self
                .get_stored(&channel_id)
                .await?
                .map(|stored| stored.accounting))
        })
    }

    fn update_channel(
        &self,
        channel_id: &str,
        updater: Box<
            dyn FnOnce(
                    Option<ChannelState>,
                )
                    -> std::result::Result<Option<ChannelState>, VerificationError>
                + Send,
        >,
    ) -> Pin<
        Box<
            dyn Future<Output = std::result::Result<Option<ChannelState>, VerificationError>>
                + Send
                + '_,
        >,
    > {
        let channel_id_owned = channel_id.to_owned();
        Box::pin(async move {
            let next = self
                .update_stored(&channel_id_owned, move |current| {
                    let descriptor = current.as_ref().and_then(|row| row.descriptor.clone());
                    let lease = current.as_ref().and_then(|row| row.lease.clone());
                    let accounting = updater(current.map(|row| row.accounting))?;
                    Ok(accounting.map(|accounting| StoredChannel {
                        accounting,
                        descriptor,
                        lease,
                    }))
                })
                .await?;
            self.notify(&channel_id_owned);
            Ok(next.map(|row| row.accounting))
        })
    }

    fn wait_for_update(&self, channel_id: &str) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let mut receiver = self.sender(channel_id).subscribe();
        Box::pin(async move {
            let _ = receiver.changed().await;
        })
    }
}

type StoredUpdater = Box<
    dyn FnOnce(
            Option<StoredChannel>,
        ) -> std::result::Result<Option<StoredChannel>, VerificationError>
        + Send,
>;

async fn initialize_sqlite(path: &Path) -> Result<()> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let connection = open_sqlite(&path).map_err(|err| Error::Store(err.to_string()))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = FULL;
                 CREATE TABLE IF NOT EXISTS mpp_session_channels (
                   channel_id TEXT PRIMARY KEY NOT NULL,
                   payload TEXT NOT NULL
                 );",
            )
            .map_err(|err| Error::Store(err.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|err| Error::Store(format!("SQLite initialization task failed: {err}")))??;
    Ok(())
}

fn open_sqlite(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

fn sqlite_read(
    path: &Path,
    channel_id: &str,
) -> std::result::Result<Option<StoredChannel>, VerificationError> {
    let connection = open_sqlite(path).map_err(sqlite_error)?;
    read_row(&connection, channel_id)
}

fn sqlite_list(
    path: &Path,
) -> std::result::Result<Vec<(String, StoredChannel)>, VerificationError> {
    let connection = open_sqlite(path).map_err(sqlite_error)?;
    let mut statement = connection
        .prepare("SELECT channel_id, payload FROM mpp_session_channels")
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sqlite_error)?;
    let mut decoded = Vec::new();
    for row in rows {
        let (channel_id, payload) = row.map_err(sqlite_error)?;
        let stored = serde_json::from_str(&payload)
            .map_err(|error| store_error(format!("failed to decode channel row: {error}")))?;
        decoded.push((channel_id, stored));
    }
    Ok(decoded)
}

fn sqlite_update(
    path: &Path,
    channel_id: &str,
    updater: StoredUpdater,
) -> std::result::Result<Option<StoredChannel>, VerificationError> {
    let mut connection = open_sqlite(path).map_err(sqlite_error)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    let current = read_row(&transaction, channel_id)?;
    let next = updater(current)?;
    match &next {
        Some(row) => {
            let payload = serde_json::to_string(row)
                .map_err(|err| store_error(format!("failed to serialize channel row: {err}")))?;
            transaction
                .execute(
                    "INSERT INTO mpp_session_channels(channel_id, payload) VALUES (?1, ?2)
                     ON CONFLICT(channel_id) DO UPDATE SET payload = excluded.payload",
                    params![channel_id, payload],
                )
                .map_err(sqlite_error)?;
        }
        None => {
            transaction
                .execute(
                    "DELETE FROM mpp_session_channels WHERE channel_id = ?1",
                    params![channel_id],
                )
                .map_err(sqlite_error)?;
        }
    }
    transaction.commit().map_err(sqlite_error)?;
    Ok(next)
}

fn read_row(
    connection: &Connection,
    channel_id: &str,
) -> std::result::Result<Option<StoredChannel>, VerificationError> {
    let payload: Option<String> = connection
        .query_row(
            "SELECT payload FROM mpp_session_channels WHERE channel_id = ?1",
            params![channel_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)?;
    payload
        .map(|payload| {
            serde_json::from_str(&payload)
                .map_err(|err| store_error(format!("failed to decode channel row: {err}")))
        })
        .transpose()
}

fn sqlite_error(error: rusqlite::Error) -> VerificationError {
    store_error(format!("SQLite channel store error: {error}"))
}

fn store_error(message: impl Into<String>) -> VerificationError {
    VerificationError::network_error(message.into())
}

#[cfg(test)]
mod tests {
    use alloy::primitives::Address;
    use mpp::protocol::methods::tempo::session_method::{deduct_from_channel, ChannelState};
    use tempfile::tempdir;

    use super::*;

    fn descriptor() -> ChannelDescriptor {
        ChannelDescriptor {
            payer: format!("{:#x}", Address::repeat_byte(0x11)),
            payee: format!("{:#x}", Address::repeat_byte(0x22)),
            operator: format!("{:#x}", Address::repeat_byte(0x33)),
            token: format!("{:#x}", Address::repeat_byte(0x44)),
            salt: format!("{:#x}", alloy::primitives::B256::repeat_byte(0x55)),
            authorized_signer: format!("{:#x}", Address::repeat_byte(0x66)),
            expiring_nonce_hash: format!("{:#x}", alloy::primitives::B256::repeat_byte(0x77)),
        }
    }

    fn channel(channel_id: &str) -> ChannelState {
        ChannelState {
            channel_id: channel_id.into(),
            chain_id: 42_431,
            escrow_contract: Address::repeat_byte(0x4d),
            payer: Address::repeat_byte(0x11),
            payee: Address::repeat_byte(0x22),
            token: Address::repeat_byte(0x44),
            authorized_signer: Address::repeat_byte(0x66),
            deposit: 10_000,
            settled_on_chain: 0,
            highest_voucher_amount: 2_000,
            highest_voucher_signature: None,
            spent: 0,
            units: 0,
            finalized: false,
            closing: false,
            close_requested_at: 0,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    async fn exercise_store(config: ChannelStoreConfig) {
        let store = SessionStore::open(&config).await.expect("store");
        store
            .upsert_verified(channel("0x01"), descriptor())
            .await
            .expect("insert");

        let mut tasks = Vec::new();
        for _ in 0..4 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                deduct_from_channel(&*store, "0x01", 1_000).await
            }));
        }
        let mut successes = 0;
        for task in tasks {
            if task.await.expect("join").is_ok() {
                successes += 1;
            }
        }
        assert_eq!(successes, 2, "only funded deductions may succeed");
        assert_eq!(
            store
                .get_channel("0x01")
                .await
                .expect("read")
                .expect("channel")
                .spent,
            2_000
        );

        let lease = StreamLease {
            owner_id: "owner-a".into(),
            logical_session_id: "session-a".into(),
            client_public_key: "key-a".into(),
            expires_at_unix: 200,
        };
        store
            .acquire_lease("0x01", lease, 100)
            .await
            .expect("first lease");
        let conflict = store
            .acquire_lease(
                "0x01",
                StreamLease {
                    owner_id: "owner-b".into(),
                    logical_session_id: "session-b".into(),
                    client_public_key: "key-b".into(),
                    expires_at_unix: 300,
                },
                150,
            )
            .await;
        assert!(conflict.is_err());

        assert!(store
            .reconcile_startup_leases(150)
            .await
            .expect("scan live lease")
            .is_empty());
        let expired = store
            .reconcile_startup_leases(201)
            .await
            .expect("scan expired lease");
        assert_eq!(expired.len(), 1);
        assert!(store
            .claim_expired_lease("0x01", "owner-a", "cleanup", 201, 260)
            .await
            .expect("claim expired lease"));
        assert!(store
            .acquire_lease(
                "0x01",
                StreamLease {
                    owner_id: "owner-c".into(),
                    logical_session_id: "session-c".into(),
                    client_public_key: "key-c".into(),
                    expires_at_unix: 300,
                },
                220,
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn memory_store_is_atomic() {
        exercise_store(ChannelStoreConfig::Memory).await;
    }

    #[tokio::test]
    async fn sqlite_store_is_atomic_and_durable() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("channels.sqlite");
        exercise_store(ChannelStoreConfig::Sqlite(path.clone())).await;
        let reopened = SessionStore::open(&ChannelStoreConfig::Sqlite(path))
            .await
            .expect("reopen");
        assert_eq!(
            reopened
                .get_channel("0x01")
                .await
                .expect("read")
                .expect("channel")
                .spent,
            2_000
        );
    }
}
