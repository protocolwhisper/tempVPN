use std::{net::Ipv4Addr, path::Path, sync::Arc, time::Duration};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::{
    config::NodeStateStoreConfig,
    error::{Error, Result},
    sessions::Session,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub session: Session,
    pub ip: Option<Ipv4Addr>,
    pub pending_peer_public_key: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditEvent {
    pub event_key: String,
    pub event_type: String,
    pub intent: Option<String>,
    pub action: Option<String>,
    pub receipt_reference: Option<String>,
    pub session_id: Option<String>,
    pub channel_id: Option<String>,
    pub amount: Option<String>,
    pub currency: Option<String>,
    pub duration_seconds: Option<u64>,
    pub remaining_seconds: Option<u64>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavePaymentResult {
    Created,
    ExistingSession(String),
}

#[derive(Debug)]
enum Backend {
    Memory,
    Sqlite(std::path::PathBuf),
}

#[derive(Clone, Debug)]
pub struct NodeStateStore {
    backend: Arc<Backend>,
    node_id: Arc<str>,
}

impl NodeStateStore {
    pub fn open(config: &NodeStateStoreConfig, node_id: &str) -> Result<Self> {
        let backend = match config {
            NodeStateStoreConfig::Memory => Backend::Memory,
            NodeStateStoreConfig::Sqlite(path) => {
                initialize_sqlite(path)?;
                Backend::Sqlite(path.clone())
            }
        };
        Ok(Self {
            backend: Arc::new(backend),
            node_id: Arc::from(node_id),
        })
    }

    pub fn is_durable(&self) -> bool {
        matches!(self.backend.as_ref(), Backend::Sqlite(_))
    }

    pub fn load_sessions(&self) -> Result<Vec<PersistedSession>> {
        let Backend::Sqlite(path) = self.backend.as_ref() else {
            return Ok(Vec::new());
        };
        let connection = open_sqlite(path)?;
        let mut statement = connection
            .prepare("SELECT payload_json FROM fixed_sessions ORDER BY session_id")
            .map_err(store_error)?;
        let payloads = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(store_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(store_error)?;
        payloads
            .into_iter()
            .map(|payload| serde_json::from_str(&payload).map_err(store_error))
            .collect()
    }

    pub async fn save_session(&self, persisted: PersistedSession) -> Result<()> {
        let Backend::Sqlite(path) = self.backend.as_ref() else {
            return Ok(());
        };
        let path = path.clone();
        tokio::task::spawn_blocking(move || save_session_sqlite(&path, &persisted))
            .await
            .map_err(|error| Error::Store(format!("node-state write task failed: {error}")))??;
        Ok(())
    }

    pub async fn save_session_and_event(
        &self,
        persisted: PersistedSession,
        event: AuditEvent,
    ) -> Result<SavePaymentResult> {
        let Backend::Sqlite(path) = self.backend.as_ref() else {
            return Ok(SavePaymentResult::Created);
        };
        let path = path.clone();
        let node_id = self.node_id.to_string();
        tokio::task::spawn_blocking(move || {
            save_session_and_event_sqlite(&path, &node_id, &persisted, &event)
        })
        .await
        .map_err(|error| Error::Store(format!("node-state transaction task failed: {error}")))?
    }

    pub async fn append_event(&self, event: AuditEvent) -> Result<bool> {
        let Backend::Sqlite(path) = self.backend.as_ref() else {
            return Ok(false);
        };
        let path = path.clone();
        let node_id = self.node_id.to_string();
        tokio::task::spawn_blocking(move || append_event_sqlite(&path, &node_id, &event))
            .await
            .map_err(|error| Error::Store(format!("audit append task failed: {error}")))?
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let Backend::Sqlite(path) = self.backend.as_ref() else {
            return Ok(());
        };
        let path = path.clone();
        let session_id = session_id.to_owned();
        tokio::task::spawn_blocking(move || {
            let connection = open_sqlite(&path)?;
            connection
                .execute(
                    "DELETE FROM fixed_sessions WHERE session_id = ?1",
                    params![session_id],
                )
                .map_err(store_error)?;
            Ok(())
        })
        .await
        .map_err(|error| Error::Store(format!("node-state delete task failed: {error}")))?
    }

    #[cfg(test)]
    pub fn audit_event(&self, event_key: &str) -> Result<Option<AuditEvent>> {
        let Backend::Sqlite(path) = self.backend.as_ref() else {
            return Ok(None);
        };
        let connection = open_sqlite(path)?;
        connection
            .query_row(
                "SELECT event_key, event_type, intent, action, receipt_reference,
                        session_id, channel_id, amount, currency, duration_seconds,
                        remaining_seconds, state
                 FROM audit_events WHERE event_key = ?1",
                params![event_key],
                |row| {
                    Ok(AuditEvent {
                        event_key: row.get(0)?,
                        event_type: row.get(1)?,
                        intent: row.get(2)?,
                        action: row.get(3)?,
                        receipt_reference: row.get(4)?,
                        session_id: row.get(5)?,
                        channel_id: row.get(6)?,
                        amount: row.get(7)?,
                        currency: row.get(8)?,
                        duration_seconds: row
                            .get::<_, Option<i64>>(9)?
                            .map(|value| value.max(0) as u64),
                        remaining_seconds: row
                            .get::<_, Option<i64>>(10)?
                            .map(|value| value.max(0) as u64),
                        state: row.get(11)?,
                    })
                },
            )
            .optional()
            .map_err(store_error)
    }
}

fn initialize_sqlite(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let connection = open_sqlite(path)?;
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             CREATE TABLE IF NOT EXISTS fixed_sessions (
               session_id TEXT PRIMARY KEY NOT NULL,
               payload_json TEXT NOT NULL,
               updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS audit_events (
               event_key TEXT PRIMARY KEY NOT NULL,
               occurred_at TEXT NOT NULL,
               node_id TEXT NOT NULL,
               event_type TEXT NOT NULL,
               intent TEXT,
               action TEXT,
               receipt_reference TEXT,
               session_id TEXT,
               channel_id TEXT,
               amount TEXT,
               currency TEXT,
               duration_seconds INTEGER,
               remaining_seconds INTEGER,
               state TEXT
             );
             CREATE INDEX IF NOT EXISTS audit_events_receipt_idx
               ON audit_events(receipt_reference);
             CREATE INDEX IF NOT EXISTS audit_events_session_idx
               ON audit_events(session_id);
             CREATE INDEX IF NOT EXISTS audit_events_channel_idx
               ON audit_events(channel_id);
             CREATE INDEX IF NOT EXISTS audit_events_occurred_idx
               ON audit_events(occurred_at);",
        )
        .map_err(store_error)
}

fn open_sqlite(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path).map_err(store_error)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(store_error)?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")
        .map_err(store_error)?;
    Ok(connection)
}

fn save_session_sqlite(path: &Path, persisted: &PersistedSession) -> Result<()> {
    let connection = open_sqlite(path)?;
    upsert_session(&connection, persisted)
}

fn save_session_and_event_sqlite(
    path: &Path,
    node_id: &str,
    persisted: &PersistedSession,
    event: &AuditEvent,
) -> Result<SavePaymentResult> {
    let mut connection = open_sqlite(path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(store_error)?;
    let inserted = insert_event(&transaction, node_id, event)?;
    if !inserted {
        let existing = transaction
            .query_row(
                "SELECT session_id FROM audit_events WHERE event_key = ?1",
                params![event.event_key],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(store_error)?
            .flatten()
            .ok_or_else(|| {
                Error::Store("duplicate fixed payment is missing its session correlation".into())
            })?;
        transaction.commit().map_err(store_error)?;
        return Ok(SavePaymentResult::ExistingSession(existing));
    }
    upsert_session(&transaction, persisted)?;
    transaction.commit().map_err(store_error)?;
    Ok(SavePaymentResult::Created)
}

fn append_event_sqlite(path: &Path, node_id: &str, event: &AuditEvent) -> Result<bool> {
    let connection = open_sqlite(path)?;
    insert_event(&connection, node_id, event)
}

fn upsert_session(connection: &Connection, persisted: &PersistedSession) -> Result<()> {
    let payload = serde_json::to_string(persisted).map_err(store_error)?;
    connection
        .execute(
            "INSERT INTO fixed_sessions (session_id, payload_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id) DO UPDATE SET
               payload_json = excluded.payload_json,
               updated_at = excluded.updated_at",
            params![
                persisted.session.session_id,
                payload,
                Utc::now().to_rfc3339()
            ],
        )
        .map_err(store_error)?;
    Ok(())
}

fn insert_event(connection: &Connection, node_id: &str, event: &AuditEvent) -> Result<bool> {
    let duration_seconds = event
        .duration_seconds
        .map(i64::try_from)
        .transpose()
        .map_err(store_error)?;
    let remaining_seconds = event
        .remaining_seconds
        .map(i64::try_from)
        .transpose()
        .map_err(store_error)?;
    let changed = connection
        .execute(
            "INSERT OR IGNORE INTO audit_events (
               event_key, occurred_at, node_id, event_type, intent, action,
               receipt_reference, session_id, channel_id, amount, currency,
               duration_seconds, remaining_seconds, state
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                event.event_key,
                Utc::now().to_rfc3339(),
                node_id,
                event.event_type,
                event.intent,
                event.action,
                event.receipt_reference,
                event.session_id,
                event.channel_id,
                event.amount,
                event.currency,
                duration_seconds,
                remaining_seconds,
                event.state,
            ],
        )
        .map_err(store_error)?;
    Ok(changed == 1)
}

fn store_error(error: impl std::fmt::Display) -> Error {
    Error::Store(error.to_string())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration as ChronoDuration, Utc};
    use tempfile::tempdir;

    use super::*;
    use crate::sessions::SessionState;

    fn session(session_id: &str) -> PersistedSession {
        let now = Utc::now();
        PersistedSession {
            session: Session {
                session_id: session_id.into(),
                node_url: "https://node.test".into(),
                client_public_key: None,
                assigned_ip: None,
                server_public_key: "server-public-key".into(),
                endpoint: "127.0.0.1:51820".into(),
                expected_exit_ip: "127.0.0.1".into(),
                created_at: now,
                connected_at: None,
                last_heartbeat_at: None,
                not_after: now + ChronoDuration::hours(1),
                total_seconds: 60,
                remaining_seconds: 60,
                state: SessionState::Paused,
            },
            ip: None,
            pending_peer_public_key: None,
        }
    }

    fn payment_event(session_id: &str) -> AuditEvent {
        AuditEvent {
            event_key: "payment:fixed:0xreceipt".into(),
            event_type: "payment_accepted".into(),
            intent: Some("charge".into()),
            action: Some("create_session".into()),
            receipt_reference: Some("0xreceipt".into()),
            session_id: Some(session_id.into()),
            amount: Some("0.01".into()),
            currency: Some("0xcurrency".into()),
            duration_seconds: Some(60),
            remaining_seconds: Some(60),
            state: Some("paused".into()),
            ..AuditEvent::default()
        }
    }

    #[tokio::test]
    async fn fixed_payment_session_and_audit_are_atomic_and_idempotent() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("node-state.sqlite");
        let store = NodeStateStore::open(&NodeStateStoreConfig::Sqlite(path), "belgium").unwrap();

        let created = store
            .save_session_and_event(session("sess_first"), payment_event("sess_first"))
            .await
            .unwrap();
        assert_eq!(created, SavePaymentResult::Created);

        let duplicate = store
            .save_session_and_event(session("sess_duplicate"), payment_event("sess_duplicate"))
            .await
            .unwrap();
        assert_eq!(
            duplicate,
            SavePaymentResult::ExistingSession("sess_first".into())
        );

        let loaded = store.load_sessions().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].session.session_id, "sess_first");
        let audit = store
            .audit_event("payment:fixed:0xreceipt")
            .unwrap()
            .unwrap();
        assert_eq!(audit.receipt_reference.as_deref(), Some("0xreceipt"));
        assert_eq!(audit.session_id.as_deref(), Some("sess_first"));
    }

    #[test]
    fn audit_schema_has_no_credential_or_key_columns() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("node-state.sqlite");
        NodeStateStore::open(&NodeStateStoreConfig::Sqlite(path.clone()), "belgium").unwrap();
        let connection = open_sqlite(&path).unwrap();
        let mut statement = connection
            .prepare("SELECT name FROM pragma_table_info('audit_events') ORDER BY cid")
            .unwrap();
        let columns = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        for forbidden in [
            "authorization",
            "credential",
            "signature",
            "transaction",
            "private_key",
            "wireguard_key",
        ] {
            assert!(!columns.iter().any(|column| column.contains(forbidden)));
        }
    }
}
