use std::{
    collections::HashSet, net::Ipv4Addr, path::Path, str::FromStr, sync::Arc, time::Duration,
};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    crypto::{token_lookup_hash, TokenCipher},
    types::{
        ActivationClaim, AdmissionState, DesiredPeer, DrainStatus, GenerationRegistration,
        NodeRecord, PaymentIntent, PeerSnapshot, SessionRecord, SessionState,
    },
    Error, Result,
};

const SCHEMA_VERSION: i64 = 1;

type ClaimSessionRow = (
    String,
    String,
    String,
    Option<String>,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
);
type PauseSessionRow = (String, String, String, Option<String>, Option<String>);
type UsageRow = (String, i64, Option<String>, Option<String>, String);

#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(path)?;
        configure(&connection)?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let mut connection = Connection::open_in_memory()?;
        configure(&connection)?;
        migrate(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub async fn health(&self) -> Result<()> {
        let connection = self.connection.lock().await;
        connection.query_row("SELECT 1", [], |_| Ok(()))?;
        Ok(())
    }

    pub async fn nodes(&self) -> Result<Vec<NodeRecord>> {
        let connection = self.connection.lock().await;
        let mut statement = connection.prepare(
            "SELECT logical_node, node_name, region, country_code, subdivision_code, city,
                    api_url, wireguard_endpoint, wireguard_public_key, expected_exit_ip,
                    available_slots, health_expires_at
             FROM node_generations
             WHERE admission_state = 'accepting' AND health_expires_at > ?1
             ORDER BY logical_node",
        )?;
        let now = Utc::now().to_rfc3339();
        let rows = statement.query_map(params![now], |row| {
            let lease: String = row.get(11)?;
            Ok(NodeRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                region: row.get(2)?,
                country_code: row.get(3)?,
                subdivision_code: row.get(4)?,
                city: row.get(5)?,
                api_url: row.get(6)?,
                wireguard_endpoint: row.get(7)?,
                wireguard_public_key: row.get(8)?,
                expected_exit_ip: row.get(9)?,
                accepting_sessions: true,
                available_slots: row.get(10)?,
                lease_expires_at: DateTime::parse_from_rfc3339(&lease)
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?
                    .with_timezone(&Utc),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub async fn create_enrollment_token(
        &self,
        logical_node: &str,
        generation_id: &str,
    ) -> Result<crate::types::EnrollmentTokenResponse> {
        validate_identifier("logical node", logical_node)?;
        validate_identifier("generation", generation_id)?;
        let token = format!("enroll_{}", Uuid::new_v4().simple());
        let now = Utc::now();
        let expires_at = now + chrono::Duration::minutes(10);
        let connection = self.connection.lock().await;
        connection.execute(
            "INSERT INTO enrollment_tokens (
                token_hash, logical_node, generation_id, expires_at, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                token_lookup_hash(&token).to_vec(),
                logical_node,
                generation_id,
                expires_at.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;
        Ok(crate::types::EnrollmentTokenResponse {
            enrollment_token: token,
            expires_at,
        })
    }

    pub async fn consume_enrollment_token(
        &self,
        token: &str,
        logical_node: &str,
        generation_id: &str,
    ) -> Result<()> {
        let connection = self.connection.lock().await;
        let now = Utc::now().to_rfc3339();
        let changed = connection.execute(
            "UPDATE enrollment_tokens SET consumed_at = ?5
             WHERE token_hash = ?1 AND logical_node = ?2 AND generation_id = ?3
               AND consumed_at IS NULL AND expires_at > ?4",
            params![
                token_lookup_hash(token).to_vec(),
                logical_node,
                generation_id,
                now,
                now,
            ],
        )?;
        if changed != 1 {
            return Err(Error::Forbidden);
        }
        Ok(())
    }

    pub async fn session_logical_node(&self, token: &str) -> Result<String> {
        let connection = self.connection.lock().await;
        connection
            .query_row(
                "SELECT logical_node FROM sessions WHERE token_hash = ?1",
                params![token_lookup_hash(token).to_vec()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound("session"))
    }

    pub async fn payment_intent_logical_node(&self, intent_id: &str) -> Result<String> {
        let connection = self.connection.lock().await;
        connection
            .query_row(
                "SELECT logical_node FROM payment_intents WHERE intent_id = ?1",
                params![intent_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound("payment intent"))
    }

    pub async fn register_generation(&self, registration: &GenerationRegistration) -> Result<()> {
        validate_identifier("logical node", &registration.logical_node)?;
        validate_identifier("generation", &registration.generation_id)?;
        validate_tunnel_network(&registration.tunnel_network)?;
        let now = Utc::now().to_rfc3339();
        let connection = self.connection.lock().await;
        connection.execute(
            "INSERT INTO node_generations (
                logical_node, generation_id, node_name, region, country_code,
                subdivision_code, city, api_url, wireguard_endpoint,
                wireguard_public_key, expected_exit_ip, tunnel_network,
                admission_state, available_slots, health_expires_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                'standby', ?13, ?14, ?15, ?15)
             ON CONFLICT(logical_node, generation_id) DO UPDATE SET
                node_name = excluded.node_name,
                region = excluded.region,
                country_code = excluded.country_code,
                subdivision_code = excluded.subdivision_code,
                city = excluded.city,
                api_url = excluded.api_url,
                wireguard_endpoint = excluded.wireguard_endpoint,
                wireguard_public_key = excluded.wireguard_public_key,
                expected_exit_ip = excluded.expected_exit_ip,
                tunnel_network = excluded.tunnel_network,
                available_slots = excluded.available_slots,
                health_expires_at = excluded.health_expires_at,
                updated_at = excluded.updated_at",
            params![
                registration.logical_node,
                registration.generation_id,
                registration.node_name,
                registration.region,
                registration.country_code,
                registration.subdivision_code,
                registration.city,
                registration.api_url.trim_end_matches('/'),
                registration.wireguard_endpoint,
                registration.wireguard_public_key,
                registration.expected_exit_ip,
                registration.tunnel_network,
                registration.available_slots,
                registration.health_expires_at.to_rfc3339(),
                now,
            ],
        )?;
        Ok(())
    }

    pub async fn renew_generation(
        &self,
        logical_node: &str,
        generation_id: &str,
        available_slots: u32,
        health_expires_at: DateTime<Utc>,
    ) -> Result<()> {
        let connection = self.connection.lock().await;
        let changed = connection.execute(
            "UPDATE node_generations
             SET available_slots = ?3, health_expires_at = ?4, updated_at = ?5
             WHERE logical_node = ?1 AND generation_id = ?2 AND admission_state != 'retired'",
            params![
                logical_node,
                generation_id,
                available_slots,
                health_expires_at.to_rfc3339(),
                Utc::now().to_rfc3339()
            ],
        )?;
        if changed == 0 {
            return Err(Error::NotFound("generation"));
        }
        Ok(())
    }

    pub async fn promote_generation(&self, logical_node: &str, generation_id: &str) -> Result<()> {
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let health: Option<String> = transaction
            .query_row(
                "SELECT health_expires_at FROM node_generations
                 WHERE logical_node = ?1 AND generation_id = ?2 AND admission_state != 'retired'",
                params![logical_node, generation_id],
                |row| row.get(0),
            )
            .optional()?;
        let health = health.ok_or(Error::NotFound("generation"))?;
        if parse_time(&health)? <= Utc::now() {
            return Err(Error::Conflict("generation is not healthy"));
        }
        let now = Utc::now().to_rfc3339();
        transaction.execute(
            "UPDATE node_generations SET admission_state = 'draining', updated_at = ?2
             WHERE logical_node = ?1 AND admission_state = 'accepting' AND generation_id != ?3",
            params![logical_node, now, generation_id],
        )?;
        transaction.execute(
            "UPDATE node_generations SET admission_state = 'accepting', updated_at = ?3
             WHERE logical_node = ?1 AND generation_id = ?2",
            params![logical_node, generation_id, now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub async fn drain_generation(&self, logical_node: &str, generation_id: &str) -> Result<()> {
        let connection = self.connection.lock().await;
        let changed = connection.execute(
            "UPDATE node_generations SET admission_state = 'draining', updated_at = ?3
             WHERE logical_node = ?1 AND generation_id = ?2 AND admission_state != 'retired'",
            params![logical_node, generation_id, Utc::now().to_rfc3339()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound("generation"));
        }
        Ok(())
    }

    pub async fn drain_status(
        &self,
        logical_node: &str,
        generation_id: &str,
    ) -> Result<DrainStatus> {
        let connection = self.connection.lock().await;
        let (state, desired, applied, peers): (String, i64, i64, i64) = connection
            .query_row(
                "SELECT admission_state, desired_peer_revision, applied_peer_revision, actual_peer_count
                 FROM node_generations WHERE logical_node = ?1 AND generation_id = ?2",
                params![logical_node, generation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or(Error::NotFound("generation"))?;
        let active_sessions: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sessions
             WHERE logical_node = ?1 AND active_generation_id = ?2 AND state = 'active' AND phase IS NULL",
            params![logical_node, generation_id],
            |row| row.get(0),
        )?;
        let transitional_sessions: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sessions
             WHERE logical_node = ?1 AND active_generation_id = ?2 AND phase IS NOT NULL",
            params![logical_node, generation_id],
            |row| row.get(0),
        )?;
        let admission_state = parse_admission_state(&state)?;
        let safe_to_delete = admission_state == AdmissionState::Draining
            && active_sessions == 0
            && transitional_sessions == 0
            && desired == applied
            && peers == 0;
        Ok(DrainStatus {
            logical_node: logical_node.to_string(),
            generation_id: generation_id.to_string(),
            admission_state,
            active_sessions: active_sessions as u64,
            transitional_sessions: transitional_sessions as u64,
            desired_peer_revision: desired as u64,
            applied_peer_revision: applied as u64,
            actual_peer_count: peers as u64,
            safe_to_delete,
        })
    }

    pub async fn create_payment_intent(
        &self,
        intent_id: Option<&str>,
        logical_node: &str,
        duration_seconds: u64,
        request_fingerprint: [u8; 32],
        challenge_key_version: u32,
        expires_at: DateTime<Utc>,
    ) -> Result<PaymentIntent> {
        if duration_seconds == 0 || challenge_key_version == 0 || expires_at <= Utc::now() {
            return Err(Error::Invalid("invalid payment intent"));
        }
        let duration_sql = i64::try_from(duration_seconds)
            .map_err(|_| Error::Invalid("payment duration is too large"))?;
        let intent = PaymentIntent {
            intent_id: intent_id
                .map(str::to_owned)
                .unwrap_or_else(|| format!("intent_{}", Uuid::new_v4().simple())),
            logical_node: logical_node.to_string(),
            duration_seconds,
            request_fingerprint: request_fingerprint.to_vec(),
            challenge_key_version,
            expires_at,
        };
        let connection = self.connection.lock().await;
        let accepting: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM node_generations
             WHERE logical_node = ?1 AND admission_state = 'accepting' AND health_expires_at > ?2)",
            params![logical_node, Utc::now().to_rfc3339()],
            |row| row.get(0),
        )?;
        if !accepting {
            return Err(Error::Conflict(
                "logical node has no healthy accepting generation",
            ));
        }
        let inserted = connection.execute(
            "INSERT INTO payment_intents (
                intent_id, logical_node, duration_seconds, request_fingerprint,
                challenge_key_version, expires_at, state, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)
             ON CONFLICT(intent_id) DO NOTHING",
            params![
                intent.intent_id,
                intent.logical_node,
                duration_sql,
                intent.request_fingerprint,
                intent.challenge_key_version,
                intent.expires_at.to_rfc3339(),
                Utc::now().to_rfc3339(),
            ],
        )?;
        if inserted == 0 {
            let existing: (String, i64, Vec<u8>, i64, String) = connection.query_row(
                "SELECT logical_node, duration_seconds, request_fingerprint,
                        challenge_key_version, expires_at
                 FROM payment_intents WHERE intent_id = ?1",
                params![intent.intent_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )?;
            if existing.0 != intent.logical_node
                || existing.1 != duration_sql
                || existing.2 != intent.request_fingerprint
                || existing.3 != i64::from(intent.challenge_key_version)
                || existing.4 != intent.expires_at.to_rfc3339()
            {
                return Err(Error::Conflict(
                    "payment intent ID belongs to another request",
                ));
            }
        }
        Ok(intent)
    }

    pub async fn redeem_payment(
        &self,
        intent_id: &str,
        payment_method: &str,
        transaction_reference: &str,
        request_fingerprint: [u8; 32],
        grace_seconds: u64,
        cipher: &TokenCipher,
    ) -> Result<SessionRecord> {
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        let existing: Option<(Vec<u8>, String)> = transaction
            .query_row(
                "SELECT request_fingerprint, session_pk FROM payment_redemptions
                 WHERE payment_method = ?1 AND transaction_reference = ?2",
                params![payment_method, transaction_reference],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((existing_fingerprint, session_pk)) = existing {
            if existing_fingerprint != request_fingerprint {
                return Err(Error::Conflict(
                    "payment transaction was redeemed for another request",
                ));
            }
            return read_session_by_pk(&transaction, &session_pk, cipher);
        }

        let intent: Option<(String, i64, Vec<u8>, String, String)> = transaction
            .query_row(
                "SELECT logical_node, duration_seconds, request_fingerprint, expires_at, state
                 FROM payment_intents WHERE intent_id = ?1",
                params![intent_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let (logical_node, duration_seconds_sql, expected_fingerprint, expires_at, state) =
            intent.ok_or(Error::NotFound("payment intent"))?;
        let duration_seconds = u64::try_from(duration_seconds_sql)
            .map_err(|_| Error::Invalid("stored payment duration"))?;
        let duration_sql = duration_seconds_sql;
        if expected_fingerprint != request_fingerprint {
            return Err(Error::Conflict("payment intent fingerprint does not match"));
        }
        if state != "pending" || parse_time(&expires_at)? <= Utc::now() {
            return Err(Error::Conflict("payment intent is not redeemable"));
        }
        let node_url: String = transaction
            .query_row(
                "SELECT api_url FROM node_generations
                 WHERE logical_node = ?1 AND admission_state = 'accepting' AND health_expires_at > ?2",
                params![logical_node, Utc::now().to_rfc3339()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::Conflict("logical node has no healthy accepting generation"))?;

        let token = format!("sess_{}", Uuid::new_v4().simple());
        let encrypted = cipher.encrypt(&token)?;
        let session_pk = Uuid::new_v4().to_string();
        let now = Utc::now();
        let grace_deadline = now + chrono::Duration::seconds(grace_seconds.max(1) as i64);
        transaction.execute(
            "INSERT INTO sessions (
                session_pk, token_hash, token_ciphertext, token_nonce, token_key_version,
                logical_node, node_url, state, total_seconds, remaining_seconds,
                grace_deadline, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'paused', ?8, ?8, ?9, ?10, ?10)",
            params![
                session_pk,
                token_lookup_hash(&token).to_vec(),
                encrypted.ciphertext,
                encrypted.nonce,
                encrypted.key_version,
                logical_node,
                node_url,
                duration_sql,
                grace_deadline.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO payment_redemptions (
                payment_method, transaction_reference, request_fingerprint,
                intent_id, session_pk, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                payment_method,
                transaction_reference,
                request_fingerprint.to_vec(),
                intent_id,
                session_pk,
                now.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "UPDATE payment_intents SET state = 'redeemed', session_pk = ?2 WHERE intent_id = ?1",
            params![intent_id, session_pk],
        )?;
        transaction.commit()?;
        Ok(SessionRecord {
            session_id: token,
            logical_node,
            node_url,
            state: SessionState::Paused,
            phase: None,
            total_seconds: duration_seconds,
            remaining_seconds: duration_seconds,
            created_at: now,
            connected_at: None,
            last_heartbeat_at: None,
            grace_deadline,
            assigned_ip: None,
            client_public_key: None,
            active_generation_id: None,
        })
    }

    pub async fn session_status(&self, token: &str, cipher: &TokenCipher) -> Result<SessionRecord> {
        let hash = token_lookup_hash(token);
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        refresh_usage(&transaction, &hash, Utc::now(), false)?;
        let session = read_session_by_hash(&transaction, &hash, cipher)?;
        transaction.commit()?;
        Ok(session)
    }

    pub async fn heartbeat_session(
        &self,
        token: &str,
        cipher: &TokenCipher,
    ) -> Result<SessionRecord> {
        let hash = token_lookup_hash(token);
        let now = Utc::now();
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        refresh_usage(&transaction, &hash, now, false)?;
        let changed = transaction.execute(
            "UPDATE sessions SET last_heartbeat_at = ?2, updated_at = ?2, revision = revision + 1
             WHERE token_hash = ?1 AND state = 'active' AND phase IS NULL",
            params![hash.to_vec(), now.to_rfc3339()],
        )?;
        if changed == 0 {
            return Err(Error::Conflict("session is not active"));
        }
        transaction.execute(
            "UPDATE desired_peers SET lease_expires_at = ?2
             WHERE session_pk = (SELECT session_pk FROM sessions WHERE token_hash = ?1)",
            params![
                hash.to_vec(),
                (now + chrono::Duration::seconds(90)).to_rfc3339()
            ],
        )?;
        let session = read_session_by_hash(&transaction, &hash, cipher)?;
        transaction.commit()?;
        Ok(session)
    }

    pub async fn claim_session(
        &self,
        token: &str,
        client_public_key: &str,
        target_logical_node: &str,
        generation_id: &str,
        cipher: &TokenCipher,
    ) -> Result<ActivationClaim> {
        if client_public_key.trim().is_empty() {
            return Err(Error::Invalid("client public key"));
        }
        validate_identifier("logical node", target_logical_node)?;
        let hash = token_lookup_hash(token);
        let now = Utc::now();
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        complete_expired_releases(&transaction, now)?;
        refresh_usage(&transaction, &hash, now, false)?;
        let session: Option<ClaimSessionRow> = transaction
            .query_row(
                "SELECT session_pk, logical_node, state, phase, remaining_seconds,
                            grace_deadline, assigned_ip, active_generation_id,
                            client_public_key, transition_revision
                     FROM sessions WHERE token_hash = ?1",
                params![hash.to_vec()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ))
                },
            )
            .optional()?;
        let (
            session_pk,
            logical_node,
            state,
            phase,
            remaining,
            grace,
            assigned_ip,
            active_generation,
            stored_public_key,
            transition_revision,
        ) = session.ok_or(Error::NotFound("session"))?;
        let is_idempotent_claim = (state == "active" && phase.is_none()
            || phase.as_deref() == Some("activating"))
            && logical_node == target_logical_node
            && active_generation.as_deref() == Some(generation_id)
            && stored_public_key.as_deref() == Some(client_public_key);
        if is_idempotent_claim {
            let generation: (String, String, String, i64) = transaction
                .query_row(
                    "SELECT wireguard_endpoint, wireguard_public_key, expected_exit_ip,
                            desired_peer_revision
                     FROM node_generations WHERE logical_node = ?1 AND generation_id = ?2",
                    params![logical_node, generation_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?
                .ok_or(Error::NotFound("generation"))?;
            let desired_revision = transition_revision.unwrap_or(generation.3);
            let session = read_session_by_hash(&transaction, &hash, cipher)?;
            transaction.commit()?;
            return Ok(ActivationClaim {
                session,
                generation_id: generation_id.to_string(),
                wireguard_endpoint: generation.0,
                wireguard_public_key: generation.1,
                expected_exit_ip: generation.2,
                desired_revision: desired_revision as u64,
            });
        }
        if state != "paused" || phase.is_some() || remaining <= 0 || parse_time(&grace)? <= now {
            return Err(Error::Conflict("session is not currently connectable"));
        }
        let generation: Option<(String, String, String, String, String)> = transaction
            .query_row(
                "SELECT wireguard_endpoint, wireguard_public_key, expected_exit_ip,
                        tunnel_network, api_url
                 FROM node_generations WHERE logical_node = ?1 AND generation_id = ?2
                   AND admission_state = 'accepting' AND health_expires_at > ?3",
                params![target_logical_node, generation_id, now.to_rfc3339()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let (wireguard_endpoint, wireguard_public_key, expected_exit_ip, tunnel_network, node_url) =
            generation.ok_or(Error::Conflict("generation is not accepting sessions"))?;
        let migrating = logical_node != target_logical_node;
        if migrating {
            transaction.execute(
                "UPDATE sessions SET logical_node = ?2, node_url = ?3, assigned_ip = NULL,
                        updated_at = ?4, revision = revision + 1 WHERE session_pk = ?1",
                params![session_pk, target_logical_node, node_url, now.to_rfc3339()],
            )?;
        }
        let assigned_ip = if migrating { None } else { assigned_ip };
        let address_was_new = assigned_ip.is_none();
        let assigned_ip = match assigned_ip {
            Some(address) => address,
            None => allocate_address(&transaction, target_logical_node, &tunnel_network)?,
        };
        transaction.execute(
            "UPDATE node_generations SET desired_peer_revision = desired_peer_revision + 1,
                updated_at = ?3 WHERE logical_node = ?1 AND generation_id = ?2",
            params![target_logical_node, generation_id, now.to_rfc3339()],
        )?;
        let desired_revision: i64 = transaction.query_row(
            "SELECT desired_peer_revision FROM node_generations
             WHERE logical_node = ?1 AND generation_id = ?2",
            params![target_logical_node, generation_id],
            |row| row.get(0),
        )?;
        let lease = now + chrono::Duration::seconds(90);
        transaction.execute(
            "INSERT INTO desired_peers (
                logical_node, generation_id, session_pk, client_public_key,
                assigned_ip, lease_expires_at, desired_revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                target_logical_node,
                generation_id,
                session_pk,
                client_public_key,
                assigned_ip,
                lease.to_rfc3339(),
                desired_revision,
            ],
        )?;
        transaction.execute(
            "UPDATE sessions SET phase = 'activating', active_generation_id = ?2,
                assigned_ip = ?3, client_public_key = ?4, transition_revision = ?5,
                address_was_new = ?6, release_after = NULL, updated_at = ?7,
                revision = revision + 1 WHERE session_pk = ?1",
            params![
                session_pk,
                generation_id,
                assigned_ip,
                client_public_key,
                desired_revision,
                address_was_new,
                now.to_rfc3339(),
            ],
        )?;
        let session = read_session_by_hash(&transaction, &hash, cipher)?;
        transaction.commit()?;
        Ok(ActivationClaim {
            session,
            generation_id: generation_id.to_string(),
            wireguard_endpoint,
            wireguard_public_key,
            expected_exit_ip,
            desired_revision: desired_revision as u64,
        })
    }

    pub async fn fail_activation(&self, token: &str) -> Result<()> {
        let hash = token_lookup_hash(token);
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let row: Option<(String, String, String, bool)> = transaction
            .query_row(
                "SELECT session_pk, logical_node, active_generation_id, address_was_new
                 FROM sessions WHERE token_hash = ?1 AND phase = 'activating'",
                params![hash.to_vec()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (session_pk, logical_node, generation_id, address_was_new) =
            row.ok_or(Error::Conflict("session is not activating"))?;
        transaction.execute(
            "DELETE FROM desired_peers WHERE session_pk = ?1",
            params![session_pk],
        )?;
        transaction.execute(
            "UPDATE node_generations SET desired_peer_revision = desired_peer_revision + 1,
                updated_at = ?3 WHERE logical_node = ?1 AND generation_id = ?2",
            params![logical_node, generation_id, now],
        )?;
        transaction.execute(
            "UPDATE sessions SET phase = NULL, active_generation_id = NULL,
                assigned_ip = CASE WHEN ?2 THEN NULL ELSE assigned_ip END,
                client_public_key = NULL, transition_revision = NULL,
                address_was_new = 0, updated_at = ?3, revision = revision + 1
             WHERE session_pk = ?1",
            params![session_pk, address_was_new, now],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub async fn pause_session(&self, token: &str, cipher: &TokenCipher) -> Result<SessionRecord> {
        let hash = token_lookup_hash(token);
        let now = Utc::now();
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        complete_expired_releases(&transaction, now)?;
        refresh_usage(&transaction, &hash, now, false)?;
        let row: Option<PauseSessionRow> = transaction
            .query_row(
                "SELECT session_pk, logical_node, state, phase, active_generation_id
                 FROM sessions WHERE token_hash = ?1",
                params![hash.to_vec()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let (session_pk, logical_node, state, phase, generation_id) =
            row.ok_or(Error::NotFound("session"))?;
        if phase.as_deref() == Some("releasing")
            || ((state == "paused" || state == "expired") && generation_id.is_none())
        {
            let session = read_session_by_hash(&transaction, &hash, cipher)?;
            transaction.commit()?;
            return Ok(session);
        }
        if phase.as_deref() == Some("activating") {
            return Err(Error::Conflict("session activation is still pending"));
        }
        let Some(generation_id) = generation_id else {
            let session = read_session_by_hash(&transaction, &hash, cipher)?;
            transaction.commit()?;
            return Ok(session);
        };
        transaction.execute(
            "DELETE FROM desired_peers WHERE session_pk = ?1",
            params![session_pk],
        )?;
        transaction.execute(
            "UPDATE node_generations SET desired_peer_revision = desired_peer_revision + 1,
                updated_at = ?3 WHERE logical_node = ?1 AND generation_id = ?2",
            params![logical_node, generation_id, now.to_rfc3339()],
        )?;
        let revision: i64 = transaction.query_row(
            "SELECT desired_peer_revision FROM node_generations
             WHERE logical_node = ?1 AND generation_id = ?2",
            params![logical_node, generation_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            "UPDATE sessions SET state = CASE WHEN remaining_seconds = 0 THEN 'expired' ELSE 'paused' END,
                phase = 'releasing', transition_revision = ?2,
                release_after = ?3, accounting_at = NULL, last_heartbeat_at = NULL,
                address_was_new = 0, updated_at = ?4, revision = revision + 1
             WHERE session_pk = ?1",
            params![
                session_pk,
                revision,
                (now + chrono::Duration::seconds(90)).to_rfc3339(),
                now.to_rfc3339()
            ],
        )?;
        let session = read_session_by_hash(&transaction, &hash, cipher)?;
        transaction.commit()?;
        Ok(session)
    }

    pub async fn sweep_stale_sessions(&self, now: DateTime<Utc>) -> Result<usize> {
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        complete_expired_releases(&transaction, now)?;
        let hashes = {
            let mut statement = transaction.prepare(
                "SELECT token_hash FROM sessions
                 WHERE state = 'active' AND phase IS NULL AND last_heartbeat_at <= ?1",
            )?;
            let rows = statement
                .query_map(
                    params![(now - chrono::Duration::seconds(90)).to_rfc3339()],
                    |row| row.get::<_, Vec<u8>>(0),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        for hash in &hashes {
            let hash: [u8; 32] = hash
                .as_slice()
                .try_into()
                .map_err(|_| Error::Invalid("stored session token hash"))?;
            refresh_usage(&transaction, &hash, now, true)?;
            begin_release_for_hash(&transaction, &hash, now, false)?;
        }
        transaction.commit()?;
        Ok(hashes.len())
    }

    pub async fn peer_snapshot(
        &self,
        logical_node: &str,
        generation_id: &str,
        cipher: &TokenCipher,
    ) -> Result<PeerSnapshot> {
        let connection = self.connection.lock().await;
        let revision: i64 = connection
            .query_row(
                "SELECT desired_peer_revision FROM node_generations
                 WHERE logical_node = ?1 AND generation_id = ?2",
                params![logical_node, generation_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound("generation"))?;
        let mut statement = connection.prepare(
            "SELECT s.token_key_version, s.token_nonce, s.token_ciphertext,
                    p.client_public_key, p.assigned_ip, p.lease_expires_at
             FROM desired_peers p JOIN sessions s ON s.session_pk = p.session_pk
             WHERE p.logical_node = ?1 AND p.generation_id = ?2 ORDER BY p.session_pk",
        )?;
        let rows = statement.query_map(params![logical_node, generation_id], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut peers = Vec::new();
        for row in rows {
            let row = row?;
            peers.push(DesiredPeer {
                session_id: cipher.decrypt(&crate::crypto::EncryptedToken {
                    key_version: row.0,
                    nonce: row.1,
                    ciphertext: row.2,
                })?,
                client_public_key: row.3,
                assigned_ip: row.4,
                lease_expires_at: parse_time(&row.5)?,
            });
        }
        Ok(PeerSnapshot {
            logical_node: logical_node.to_string(),
            generation_id: generation_id.to_string(),
            revision: revision as u64,
            peers,
        })
    }

    pub async fn acknowledge_peers(
        &self,
        logical_node: &str,
        generation_id: &str,
        applied_revision: u64,
        actual_peer_count: u64,
    ) -> Result<()> {
        let applied_revision =
            i64::try_from(applied_revision).map_err(|_| Error::Invalid("applied peer revision"))?;
        let actual_peer_count =
            i64::try_from(actual_peer_count).map_err(|_| Error::Invalid("actual peer count"))?;
        let now = Utc::now();
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let desired: i64 = transaction
            .query_row(
                "SELECT desired_peer_revision FROM node_generations
                 WHERE logical_node = ?1 AND generation_id = ?2",
                params![logical_node, generation_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound("generation"))?;
        if applied_revision > desired {
            return Err(Error::Conflict("applied revision exceeds desired revision"));
        }
        transaction.execute(
            "UPDATE node_generations SET applied_peer_revision = MAX(applied_peer_revision, ?3),
                actual_peer_count = ?4, updated_at = ?5
             WHERE logical_node = ?1 AND generation_id = ?2",
            params![
                logical_node,
                generation_id,
                applied_revision,
                actual_peer_count,
                now.to_rfc3339()
            ],
        )?;
        transaction.execute(
            "UPDATE sessions SET state = 'active', phase = NULL,
                accounting_at = ?4, last_heartbeat_at = ?4, transition_revision = NULL,
                address_was_new = 0, updated_at = ?4, revision = revision + 1
             WHERE logical_node = ?1 AND active_generation_id = ?2 AND phase = 'activating'
               AND transition_revision <= ?3",
            params![
                logical_node,
                generation_id,
                applied_revision,
                now.to_rfc3339()
            ],
        )?;
        complete_acknowledged_releases(
            &transaction,
            logical_node,
            generation_id,
            applied_revision,
            now,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub async fn terminate_session(
        &self,
        token: &str,
        cipher: &TokenCipher,
    ) -> Result<SessionRecord> {
        let hash = token_lookup_hash(token);
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        refresh_usage(&transaction, &hash, Utc::now(), false)?;
        transaction.execute(
            "DELETE FROM desired_peers WHERE session_pk = (SELECT session_pk FROM sessions WHERE token_hash = ?1)",
            params![hash.to_vec()],
        )?;
        let changed = transaction.execute(
            "UPDATE sessions SET state = 'expired', phase = NULL, remaining_seconds = 0,
                assigned_ip = NULL, client_public_key = NULL, active_generation_id = NULL,
                release_after = NULL, accounting_at = NULL, last_heartbeat_at = NULL,
                updated_at = ?2, revision = revision + 1
             WHERE token_hash = ?1",
            params![hash.to_vec(), Utc::now().to_rfc3339()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound("session"));
        }
        let session = read_session_by_hash(&transaction, &hash, cipher)?;
        transaction.commit()?;
        Ok(session)
    }

    pub async fn allocate_address(
        &self,
        logical_node: &str,
        tunnel_network: &str,
    ) -> Result<String> {
        let mut connection = self.connection.lock().await;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let address = allocate_address(&transaction, logical_node, tunnel_network)?;
        transaction.rollback()?;
        Ok(address)
    }

    #[cfg(test)]
    pub(crate) async fn schema_version(&self) -> Result<i64> {
        let connection = self.connection.lock().await;
        connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(Into::into)
    }
}

fn complete_acknowledged_releases(
    transaction: &rusqlite::Transaction<'_>,
    logical_node: &str,
    generation_id: &str,
    applied_revision: i64,
    now: DateTime<Utc>,
) -> Result<()> {
    transaction.execute(
        "UPDATE sessions SET phase = NULL, active_generation_id = NULL,
            client_public_key = NULL, release_after = NULL, transition_revision = NULL,
            assigned_ip = CASE WHEN state = 'expired' THEN NULL ELSE assigned_ip END,
            updated_at = ?4, revision = revision + 1
         WHERE logical_node = ?1 AND active_generation_id = ?2 AND phase = 'releasing'
           AND transition_revision <= ?3",
        params![
            logical_node,
            generation_id,
            applied_revision,
            now.to_rfc3339()
        ],
    )?;
    Ok(())
}

fn begin_release_for_hash(
    transaction: &rusqlite::Transaction<'_>,
    token_hash: &[u8; 32],
    now: DateTime<Utc>,
    force_expired: bool,
) -> Result<()> {
    let row: Option<(String, String, String, String)> = transaction
        .query_row(
            "SELECT session_pk, logical_node, active_generation_id, state
             FROM sessions WHERE token_hash = ?1 AND active_generation_id IS NOT NULL",
            params![token_hash.to_vec()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((session_pk, logical_node, generation_id, state)) = row else {
        return Ok(());
    };
    transaction.execute(
        "DELETE FROM desired_peers WHERE session_pk = ?1",
        params![session_pk],
    )?;
    transaction.execute(
        "UPDATE node_generations SET desired_peer_revision = desired_peer_revision + 1,
            updated_at = ?3 WHERE logical_node = ?1 AND generation_id = ?2",
        params![logical_node, generation_id, now.to_rfc3339()],
    )?;
    let revision: i64 = transaction.query_row(
        "SELECT desired_peer_revision FROM node_generations
         WHERE logical_node = ?1 AND generation_id = ?2",
        params![logical_node, generation_id],
        |row| row.get(0),
    )?;
    let terminal = force_expired || state == "expired";
    transaction.execute(
        "UPDATE sessions SET state = CASE WHEN ?2 THEN 'expired' ELSE 'paused' END,
            remaining_seconds = CASE WHEN ?2 THEN 0 ELSE remaining_seconds END,
            phase = 'releasing', transition_revision = ?3, release_after = ?4,
            accounting_at = NULL, last_heartbeat_at = NULL, address_was_new = 0,
            updated_at = ?5, revision = revision + 1 WHERE session_pk = ?1",
        params![
            session_pk,
            terminal,
            revision,
            (now + chrono::Duration::seconds(90)).to_rfc3339(),
            now.to_rfc3339()
        ],
    )?;
    Ok(())
}

fn complete_expired_releases(
    transaction: &rusqlite::Transaction<'_>,
    now: DateTime<Utc>,
) -> Result<()> {
    transaction.execute(
        "UPDATE sessions SET phase = NULL, active_generation_id = NULL,
            client_public_key = NULL, release_after = NULL, transition_revision = NULL,
            assigned_ip = CASE WHEN state = 'expired' THEN NULL ELSE assigned_ip END,
            updated_at = ?1, revision = revision + 1
         WHERE phase = 'releasing' AND release_after <= ?1",
        params![now.to_rfc3339()],
    )?;
    Ok(())
}

fn validate_identifier(label: &'static str, value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(Error::Invalid(label));
    }
    Ok(())
}

fn validate_tunnel_network(network: &str) -> Result<Ipv4Addr> {
    let (address, prefix) = network
        .split_once('/')
        .ok_or(Error::Invalid("tunnel network"))?;
    if prefix != "24" {
        return Err(Error::Invalid("tunnel network must use /24"));
    }
    let address = Ipv4Addr::from_str(address).map_err(|_| Error::Invalid("tunnel network"))?;
    if address.octets()[3] != 0 {
        return Err(Error::Invalid("tunnel network must be a /24 base address"));
    }
    Ok(address)
}

fn allocate_address(
    transaction: &rusqlite::Transaction<'_>,
    logical_node: &str,
    tunnel_network: &str,
) -> Result<String> {
    let base = validate_tunnel_network(tunnel_network)?.octets();
    let mut statement = transaction.prepare(
        "SELECT assigned_ip FROM sessions WHERE logical_node = ?1 AND assigned_ip IS NOT NULL",
    )?;
    let used = statement
        .query_map(params![logical_node], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<HashSet<_>, _>>()?;
    (2_u8..=254)
        .map(|host| Ipv4Addr::new(base[0], base[1], base[2], host).to_string())
        .find(|address| !used.contains(address))
        .ok_or(Error::Conflict(
            "logical node tunnel address pool is exhausted",
        ))
}

fn refresh_usage(
    transaction: &rusqlite::Transaction<'_>,
    token_hash: &[u8; 32],
    now: DateTime<Utc>,
    stale_cutoff: bool,
) -> Result<()> {
    let row: Option<UsageRow> = transaction
        .query_row(
            "SELECT state, remaining_seconds, accounting_at, last_heartbeat_at, grace_deadline
             FROM sessions WHERE token_hash = ?1",
            params![token_hash.to_vec()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let (state, remaining_sql, accounting_at, last_heartbeat, grace_deadline) =
        row.ok_or(Error::NotFound("session"))?;
    let remaining =
        u64::try_from(remaining_sql).map_err(|_| Error::Invalid("stored remaining balance"))?;
    let grace_deadline = parse_time(&grace_deadline)?;
    if state != "active" {
        if grace_deadline <= now && state != "expired" {
            transaction.execute(
                "UPDATE sessions SET state = 'expired', remaining_seconds = 0,
                    assigned_ip = NULL, updated_at = ?2, revision = revision + 1
                 WHERE token_hash = ?1",
                params![token_hash.to_vec(), now.to_rfc3339()],
            )?;
        }
        return Ok(());
    }
    let start = accounting_at
        .as_deref()
        .map(parse_time)
        .transpose()?
        .unwrap_or(now);
    let end = if stale_cutoff {
        last_heartbeat
            .as_deref()
            .map(parse_time)
            .transpose()?
            .map(|heartbeat| heartbeat + chrono::Duration::seconds(90))
            .filter(|cutoff| *cutoff < now)
            .unwrap_or(now)
    } else {
        now
    };
    let elapsed = (end - start).num_seconds().max(0) as u64;
    let remaining = remaining.saturating_sub(elapsed);
    let expired = remaining == 0 || grace_deadline <= now;
    transaction.execute(
        "UPDATE sessions SET remaining_seconds = ?2, accounting_at = ?3,
            state = CASE WHEN ?4 THEN 'expired' ELSE state END,
            updated_at = ?5, revision = revision + 1
         WHERE token_hash = ?1",
        params![
            token_hash.to_vec(),
            i64::try_from(if expired { 0 } else { remaining })
                .map_err(|_| Error::Invalid("remaining balance is too large"))?,
            end.to_rfc3339(),
            expired,
            now.to_rfc3339()
        ],
    )?;
    if expired {
        begin_release_for_hash(transaction, token_hash, now, true)?;
    }
    Ok(())
}

fn read_session_by_hash(
    transaction: &rusqlite::Transaction<'_>,
    token_hash: &[u8; 32],
    cipher: &TokenCipher,
) -> Result<SessionRecord> {
    let session_pk: String = transaction
        .query_row(
            "SELECT session_pk FROM sessions WHERE token_hash = ?1",
            params![token_hash.to_vec()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound("session"))?;
    read_session_by_pk(transaction, &session_pk, cipher)
}

fn read_session_by_pk(
    transaction: &rusqlite::Transaction<'_>,
    session_pk: &str,
    cipher: &TokenCipher,
) -> Result<SessionRecord> {
    let row = transaction
        .query_row(
            "SELECT token_key_version, token_nonce, token_ciphertext, logical_node,
                    node_url, state, phase, total_seconds, remaining_seconds,
                    grace_deadline, assigned_ip, client_public_key, active_generation_id,
                    created_at, accounting_at, last_heartbeat_at
             FROM sessions WHERE session_pk = ?1",
            params![session_pk],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                ))
            },
        )
        .optional()?
        .ok_or(Error::NotFound("session"))?;
    let token = cipher.decrypt(&crate::crypto::EncryptedToken {
        key_version: row.0,
        nonce: row.1,
        ciphertext: row.2,
    })?;
    Ok(SessionRecord {
        session_id: token,
        logical_node: row.3,
        node_url: row.4,
        state: parse_session_state(&row.5)?,
        phase: row.6,
        total_seconds: u64::try_from(row.7).map_err(|_| Error::Invalid("stored total balance"))?,
        remaining_seconds: u64::try_from(row.8)
            .map_err(|_| Error::Invalid("stored remaining balance"))?,
        created_at: parse_time(&row.13)?,
        connected_at: row.14.as_deref().map(parse_time).transpose()?,
        last_heartbeat_at: row.15.as_deref().map(parse_time).transpose()?,
        grace_deadline: parse_time(&row.9)?,
        assigned_ip: row.10,
        client_public_key: row.11,
        active_generation_id: row.12,
    })
}

fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| Error::Invalid("stored timestamp"))
}

fn parse_admission_state(value: &str) -> Result<AdmissionState> {
    match value {
        "standby" => Ok(AdmissionState::Standby),
        "accepting" => Ok(AdmissionState::Accepting),
        "draining" => Ok(AdmissionState::Draining),
        "retired" => Ok(AdmissionState::Retired),
        _ => Err(Error::Invalid("stored admission state")),
    }
}

fn parse_session_state(value: &str) -> Result<SessionState> {
    match value {
        "paused" => Ok(SessionState::Paused),
        "active" => Ok(SessionState::Active),
        "expired" => Ok(SessionState::Expired),
        _ => Err(Error::Invalid("stored session state")),
    }
}

fn configure(connection: &Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;",
    )?;
    Ok(())
}

fn migrate(connection: &mut Connection) -> Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(Error::UnsupportedSchema {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }

    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "CREATE TABLE node_generations (
            logical_node TEXT NOT NULL,
            generation_id TEXT NOT NULL,
            node_name TEXT NOT NULL,
            region TEXT NOT NULL,
            country_code TEXT,
            subdivision_code TEXT,
            city TEXT,
            api_url TEXT NOT NULL,
            wireguard_endpoint TEXT NOT NULL,
            wireguard_public_key TEXT NOT NULL,
            expected_exit_ip TEXT NOT NULL,
            tunnel_network TEXT NOT NULL,
            admission_state TEXT NOT NULL CHECK (admission_state IN ('standby','accepting','draining','retired')),
            available_slots INTEGER NOT NULL DEFAULT 0 CHECK (available_slots >= 0),
            health_expires_at TEXT NOT NULL,
            desired_peer_revision INTEGER NOT NULL DEFAULT 0 CHECK (desired_peer_revision >= 0),
            applied_peer_revision INTEGER NOT NULL DEFAULT 0 CHECK (applied_peer_revision >= 0),
            actual_peer_count INTEGER NOT NULL DEFAULT 0 CHECK (actual_peer_count >= 0),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (logical_node, generation_id)
         );
         CREATE UNIQUE INDEX one_accepting_generation_per_node
            ON node_generations(logical_node) WHERE admission_state = 'accepting';

         CREATE TABLE sessions (
            session_pk TEXT PRIMARY KEY,
            token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
            token_ciphertext BLOB NOT NULL,
            token_nonce BLOB NOT NULL CHECK (length(token_nonce) = 12),
            token_key_version INTEGER NOT NULL CHECK (token_key_version > 0),
            logical_node TEXT NOT NULL,
            node_url TEXT NOT NULL,
            state TEXT NOT NULL CHECK (state IN ('paused','active','expired')),
            phase TEXT CHECK (phase IN ('activating','releasing')),
            total_seconds INTEGER NOT NULL CHECK (total_seconds > 0),
            remaining_seconds INTEGER NOT NULL CHECK (remaining_seconds >= 0),
            grace_deadline TEXT NOT NULL,
            accounting_at TEXT,
            last_heartbeat_at TEXT,
            assigned_ip TEXT,
            client_public_key TEXT,
            active_generation_id TEXT,
            release_after TEXT,
            transition_revision INTEGER,
            address_was_new INTEGER NOT NULL DEFAULT 0 CHECK (address_was_new IN (0,1)),
            revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (logical_node, active_generation_id)
              REFERENCES node_generations(logical_node, generation_id)
         );
         CREATE UNIQUE INDEX unique_logical_node_tunnel_address
            ON sessions(logical_node, assigned_ip) WHERE assigned_ip IS NOT NULL;
         CREATE INDEX sessions_active_generation
            ON sessions(logical_node, active_generation_id)
            WHERE active_generation_id IS NOT NULL;

         CREATE TABLE payment_intents (
            intent_id TEXT PRIMARY KEY,
            logical_node TEXT NOT NULL,
            duration_seconds INTEGER NOT NULL CHECK (duration_seconds > 0),
            request_fingerprint BLOB NOT NULL CHECK (length(request_fingerprint) = 32),
            challenge_key_version INTEGER NOT NULL CHECK (challenge_key_version > 0),
            expires_at TEXT NOT NULL,
            state TEXT NOT NULL CHECK (state IN ('pending','redeemed','expired')),
            session_pk TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (session_pk) REFERENCES sessions(session_pk)
         );

         CREATE TABLE payment_redemptions (
            payment_method TEXT NOT NULL,
            transaction_reference TEXT NOT NULL,
            request_fingerprint BLOB NOT NULL CHECK (length(request_fingerprint) = 32),
            intent_id TEXT NOT NULL,
            session_pk TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (payment_method, transaction_reference),
            FOREIGN KEY (intent_id) REFERENCES payment_intents(intent_id),
            FOREIGN KEY (session_pk) REFERENCES sessions(session_pk)
         );

         CREATE TABLE desired_peers (
            logical_node TEXT NOT NULL,
            generation_id TEXT NOT NULL,
            session_pk TEXT NOT NULL UNIQUE,
            client_public_key TEXT NOT NULL,
            assigned_ip TEXT NOT NULL,
            lease_expires_at TEXT NOT NULL,
            desired_revision INTEGER NOT NULL CHECK (desired_revision > 0),
            PRIMARY KEY (logical_node, generation_id, session_pk),
            FOREIGN KEY (logical_node, generation_id)
              REFERENCES node_generations(logical_node, generation_id) ON DELETE CASCADE,
            FOREIGN KEY (session_pk) REFERENCES sessions(session_pk)
         );

         CREATE TABLE enrollment_tokens (
            token_hash BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
            logical_node TEXT NOT NULL,
            generation_id TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            consumed_at TEXT,
            created_at TEXT NOT NULL
         );
         PRAGMA user_version = 1;",
    )?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{token_lookup_hash, EncryptedToken, TokenCipher};
    use tempfile::tempdir;

    fn generation(id: &str) -> GenerationRegistration {
        generation_for("node-a", id, "10.90.0.0/24")
    }

    fn generation_for(
        logical_node: &str,
        generation_id: &str,
        tunnel_network: &str,
    ) -> GenerationRegistration {
        GenerationRegistration {
            logical_node: logical_node.into(),
            generation_id: generation_id.into(),
            node_name: logical_node.into(),
            region: "test".into(),
            country_code: Some("US".into()),
            subdivision_code: None,
            city: Some("Test City".into()),
            api_url: format!("https://{logical_node}.test"),
            wireguard_endpoint: format!("{logical_node}-{generation_id}.test:51820"),
            wireguard_public_key: format!("{generation_id}-server-key"),
            expected_exit_ip: "192.0.2.1".into(),
            tunnel_network: tunnel_network.into(),
            available_slots: 253,
            health_expires_at: Utc::now() + chrono::Duration::minutes(5),
        }
    }

    #[tokio::test]
    async fn creates_and_reopens_versioned_schema() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("coordinator.sqlite");
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().await.unwrap(), SCHEMA_VERSION);
        drop(store);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.schema_version().await.unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn rejects_unknown_newer_schema() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("newer.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        drop(connection);
        assert!(matches!(
            Store::open(&path),
            Err(Error::UnsupportedSchema {
                found: 99,
                supported: 1
            })
        ));
    }

    #[tokio::test]
    async fn constraints_reject_duplicate_accepting_generation_and_address() {
        let store = Store::open_in_memory().unwrap();
        let connection = store.connection.lock().await;
        let now = Utc::now().to_rfc3339();
        let insert_generation = |generation: &str| {
            connection.execute(
                "INSERT INTO node_generations (
                    logical_node, generation_id, node_name, region, api_url,
                    wireguard_endpoint, wireguard_public_key, expected_exit_ip,
                    tunnel_network, admission_state, health_expires_at, created_at, updated_at
                 ) VALUES ('node-a', ?1, 'Node A', 'test', 'https://node-a.test',
                    '192.0.2.1:51820', 'server-key', '192.0.2.1', '10.90.0.0/24',
                    'accepting', ?2, ?2, ?2)",
                params![generation, now],
            )
        };
        insert_generation("blue").unwrap();
        assert!(insert_generation("green").is_err());

        for session in ["one", "two"] {
            let result = connection.execute(
                "INSERT INTO sessions (
                    session_pk, token_hash, token_ciphertext, token_nonce, token_key_version,
                    logical_node, node_url, state, total_seconds, remaining_seconds, grace_deadline,
                    assigned_ip, created_at, updated_at
                 ) VALUES (?1, ?2, X'01', zeroblob(12), 1, 'node-a', 'https://node-a.test', 'paused',
                    60, 60, ?3, '10.90.0.2', ?3, ?3)",
                params![session, token_lookup_hash(session).to_vec(), now],
            );
            if session == "one" {
                result.unwrap();
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[tokio::test]
    async fn encrypted_token_round_trips_after_database_reopen() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("tokens.sqlite");
        let cipher = TokenCipher::new(&[9_u8; 32], 2).unwrap();
        let token = "sess_durable_secret";
        let encrypted = cipher.encrypt(token).unwrap();
        {
            let store = Store::open(&path).unwrap();
            let connection = store.connection.lock().await;
            let now = Utc::now().to_rfc3339();
            connection
                .execute(
                    "INSERT INTO sessions (
                        session_pk, token_hash, token_ciphertext, token_nonce, token_key_version,
                        logical_node, node_url, state, total_seconds, remaining_seconds, grace_deadline,
                        created_at, updated_at
                     ) VALUES ('session', ?1, ?2, ?3, ?4, 'node-a', 'https://node-a.test', 'paused', 60, 60, ?5, ?5, ?5)",
                    params![
                        token_lookup_hash(token).to_vec(),
                        encrypted.ciphertext,
                        encrypted.nonce,
                        encrypted.key_version,
                        now
                    ],
                )
                .unwrap();
        }
        let reopened = Store::open(&path).unwrap();
        let connection = reopened.connection.lock().await;
        let persisted = connection
            .query_row(
                "SELECT token_key_version, token_nonce, token_ciphertext FROM sessions WHERE session_pk = 'session'",
                [],
                |row| {
                    Ok(EncryptedToken {
                        key_version: row.get(0)?,
                        nonce: row.get(1)?,
                        ciphertext: row.get(2)?,
                    })
                },
            )
            .unwrap();
        assert_eq!(cipher.decrypt(&persisted).unwrap(), token);
        assert!(!persisted
            .ciphertext
            .windows(token.len())
            .any(|window| window == token.as_bytes()));
    }

    #[tokio::test]
    async fn serializes_concurrent_immediate_writes_without_loss() {
        let store = Store::open_in_memory().unwrap();
        let mut tasks = Vec::new();
        for value in 0_u8..32 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                let mut connection = store.connection.lock().await;
                let transaction = connection
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                    .unwrap();
                let now = Utc::now().to_rfc3339();
                transaction
                    .execute(
                        "INSERT INTO enrollment_tokens (
                            token_hash, logical_node, generation_id, expires_at, created_at
                         ) VALUES (?1, 'node-a', ?2, ?3, ?3)",
                        params![vec![value; 32], format!("generation-{value}"), now],
                    )
                    .unwrap();
                transaction.commit().unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        let connection = store.connection.lock().await;
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM enrollment_tokens", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 32);
    }

    #[tokio::test]
    async fn promotes_one_generation_and_reports_empty_drain_safe() {
        let store = Store::open_in_memory().unwrap();
        store
            .register_generation(&generation("blue"))
            .await
            .unwrap();
        store
            .register_generation(&generation("green"))
            .await
            .unwrap();
        store.promote_generation("node-a", "blue").await.unwrap();
        store.promote_generation("node-a", "green").await.unwrap();

        let blue = store.drain_status("node-a", "blue").await.unwrap();
        assert_eq!(blue.admission_state, AdmissionState::Draining);
        assert!(blue.safe_to_delete);
        let nodes = store.nodes().await.unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].wireguard_public_key, "green-server-key");
    }

    #[tokio::test]
    async fn payment_retry_recovers_same_session_and_replay_mismatch_fails() {
        let store = Store::open_in_memory().unwrap();
        let cipher = TokenCipher::new(&[5_u8; 32], 1).unwrap();
        store
            .register_generation(&generation("green"))
            .await
            .unwrap();
        store.promote_generation("node-a", "green").await.unwrap();
        let fingerprint = token_lookup_hash("node-a:60");
        let intent = store
            .create_payment_intent(
                None,
                "node-a",
                60,
                fingerprint,
                1,
                Utc::now() + chrono::Duration::minutes(5),
            )
            .await
            .unwrap();
        let first = store
            .redeem_payment(
                &intent.intent_id,
                "tempo",
                "tx-1",
                fingerprint,
                3600,
                &cipher,
            )
            .await
            .unwrap();
        let retry = store
            .redeem_payment(
                &intent.intent_id,
                "tempo",
                "tx-1",
                fingerprint,
                3600,
                &cipher,
            )
            .await
            .unwrap();
        assert_eq!(first, retry);
        assert_eq!(first.state, SessionState::Paused);
        assert_eq!(first.remaining_seconds, 60);

        let mismatch = store
            .redeem_payment(
                &intent.intent_id,
                "tempo",
                "tx-1",
                token_lookup_hash("different"),
                3600,
                &cipher,
            )
            .await;
        assert!(matches!(mismatch, Err(Error::Conflict(_))));
    }

    #[tokio::test]
    async fn durable_payment_survives_reopen_and_terminal_cleanup_releases_state() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("lifecycle.sqlite");
        let cipher = TokenCipher::new(&[6_u8; 32], 1).unwrap();
        let token = {
            let store = Store::open(&path).unwrap();
            store
                .register_generation(&generation("green"))
                .await
                .unwrap();
            store.promote_generation("node-a", "green").await.unwrap();
            let fingerprint = token_lookup_hash("node-a:120");
            let intent = store
                .create_payment_intent(
                    None,
                    "node-a",
                    120,
                    fingerprint,
                    1,
                    Utc::now() + chrono::Duration::minutes(5),
                )
                .await
                .unwrap();
            store
                .redeem_payment(
                    &intent.intent_id,
                    "tempo",
                    "tx-durable",
                    fingerprint,
                    3600,
                    &cipher,
                )
                .await
                .unwrap()
                .session_id
        };
        let reopened = Store::open(&path).unwrap();
        let status = reopened.session_status(&token, &cipher).await.unwrap();
        assert_eq!(status.remaining_seconds, 120);
        let terminated = reopened.terminate_session(&token, &cipher).await.unwrap();
        assert_eq!(terminated.state, SessionState::Expired);
        assert_eq!(terminated.remaining_seconds, 0);
        assert_eq!(terminated.assigned_ip, None);
    }

    #[tokio::test]
    async fn address_allocator_skips_durable_paused_reservations() {
        let store = Store::open_in_memory().unwrap();
        let connection = store.connection.lock().await;
        let now = Utc::now().to_rfc3339();
        connection
            .execute(
                "INSERT INTO sessions (
                    session_pk, token_hash, token_ciphertext, token_nonce, token_key_version,
                    logical_node, node_url, state, total_seconds, remaining_seconds,
                    grace_deadline, assigned_ip, created_at, updated_at
                 ) VALUES ('reserved', ?1, X'01', zeroblob(12), 1, 'node-a',
                    'https://node-a.test', 'paused', 60, 60, ?2, '10.90.0.2', ?2, ?2)",
                params![token_lookup_hash("reserved").to_vec(), now],
            )
            .unwrap();
        drop(connection);
        assert_eq!(
            store
                .allocate_address("node-a", "10.90.0.0/24")
                .await
                .unwrap(),
            "10.90.0.3"
        );
    }

    #[tokio::test]
    async fn blue_active_session_drains_and_resumes_on_green_without_migration() {
        let store = Store::open_in_memory().unwrap();
        let cipher = TokenCipher::new(&[11_u8; 32], 1).unwrap();
        store
            .register_generation(&generation("blue"))
            .await
            .unwrap();
        store.promote_generation("node-a", "blue").await.unwrap();
        let fingerprint = token_lookup_hash("blue-session");
        let intent = store
            .create_payment_intent(
                None,
                "node-a",
                60,
                fingerprint,
                1,
                Utc::now() + chrono::Duration::minutes(5),
            )
            .await
            .unwrap();
        let paid = store
            .redeem_payment(
                &intent.intent_id,
                "tempo",
                "tx-blue",
                fingerprint,
                3600,
                &cipher,
            )
            .await
            .unwrap();
        let blue_claim = store
            .claim_session(&paid.session_id, "client-key", "node-a", "blue", &cipher)
            .await
            .unwrap();
        assert_eq!(blue_claim.session.phase.as_deref(), Some("activating"));
        assert_eq!(blue_claim.session.assigned_ip.as_deref(), Some("10.90.0.2"));
        store
            .acknowledge_peers("node-a", "blue", blue_claim.desired_revision, 1)
            .await
            .unwrap();

        store
            .register_generation(&generation("green"))
            .await
            .unwrap();
        store.promote_generation("node-a", "green").await.unwrap();
        let retried_blue_claim = store
            .claim_session(&paid.session_id, "client-key", "node-a", "blue", &cipher)
            .await
            .unwrap();
        assert_eq!(retried_blue_claim.session.state, SessionState::Active);
        assert_eq!(retried_blue_claim.session.phase, None);
        let heartbeat = store
            .heartbeat_session(&paid.session_id, &cipher)
            .await
            .unwrap();
        assert_eq!(heartbeat.active_generation_id.as_deref(), Some("blue"));
        assert_eq!(heartbeat.state, SessionState::Active);

        {
            let connection = store.connection.lock().await;
            connection
                .execute(
                    "UPDATE sessions SET accounting_at = ?2 WHERE token_hash = ?1",
                    params![
                        token_lookup_hash(&paid.session_id).to_vec(),
                        (Utc::now() - chrono::Duration::seconds(10)).to_rfc3339()
                    ],
                )
                .unwrap();
        }
        let accounted = store
            .session_status(&paid.session_id, &cipher)
            .await
            .unwrap();
        assert!((49..=50).contains(&accounted.remaining_seconds));

        let releasing = store
            .pause_session(&paid.session_id, &cipher)
            .await
            .unwrap();
        assert_eq!(releasing.state, SessionState::Paused);
        assert_eq!(releasing.phase.as_deref(), Some("releasing"));
        assert!(matches!(
            store
                .claim_session(&paid.session_id, "client-key", "node-a", "green", &cipher)
                .await,
            Err(Error::Conflict(_))
        ));
        let blue_snapshot = store
            .peer_snapshot("node-a", "blue", &cipher)
            .await
            .unwrap();
        assert!(blue_snapshot.peers.is_empty());
        let pending_drain = store.drain_status("node-a", "blue").await.unwrap();
        assert!(!pending_drain.safe_to_delete);
        assert_eq!(pending_drain.transitional_sessions, 1);
        store
            .acknowledge_peers("node-a", "blue", blue_snapshot.revision, 0)
            .await
            .unwrap();

        let green_claim = store
            .claim_session(
                &paid.session_id,
                "new-client-key",
                "node-a",
                "green",
                &cipher,
            )
            .await
            .unwrap();
        assert_eq!(
            green_claim.session.assigned_ip.as_deref(),
            Some("10.90.0.2")
        );
        assert_eq!(green_claim.wireguard_public_key, "green-server-key");
        assert_eq!(green_claim.wireguard_endpoint, "node-a-green.test:51820");
        store
            .acknowledge_peers("node-a", "green", green_claim.desired_revision, 1)
            .await
            .unwrap();

        let blue_drain = store.drain_status("node-a", "blue").await.unwrap();
        assert!(blue_drain.safe_to_delete);
    }

    #[tokio::test]
    async fn paused_balance_migrates_to_another_logical_node() {
        let store = Store::open_in_memory().unwrap();
        let cipher = TokenCipher::new(&[21_u8; 32], 1).unwrap();
        store
            .register_generation(&generation_for("node-a", "blue", "10.90.0.0/24"))
            .await
            .unwrap();
        store
            .register_generation(&generation_for("node-b", "green", "10.91.0.0/24"))
            .await
            .unwrap();
        store.promote_generation("node-a", "blue").await.unwrap();
        store.promote_generation("node-b", "green").await.unwrap();

        let fingerprint = token_lookup_hash("portable-session");
        let intent = store
            .create_payment_intent(
                None,
                "node-a",
                300,
                fingerprint,
                1,
                Utc::now() + chrono::Duration::minutes(5),
            )
            .await
            .unwrap();
        let paid = store
            .redeem_payment(
                &intent.intent_id,
                "tempo",
                "tx-portable",
                fingerprint,
                3600,
                &cipher,
            )
            .await
            .unwrap();
        let first = store
            .claim_session(&paid.session_id, "key-a", "node-a", "blue", &cipher)
            .await
            .unwrap();
        store
            .acknowledge_peers("node-a", "blue", first.desired_revision, 1)
            .await
            .unwrap();

        assert!(matches!(
            store
                .claim_session(&paid.session_id, "key-b", "node-b", "green", &cipher)
                .await,
            Err(Error::Conflict(_))
        ));

        store
            .pause_session(&paid.session_id, &cipher)
            .await
            .unwrap();
        let old_snapshot = store
            .peer_snapshot("node-a", "blue", &cipher)
            .await
            .unwrap();
        store
            .acknowledge_peers("node-a", "blue", old_snapshot.revision, 0)
            .await
            .unwrap();

        let migrated = store
            .claim_session(&paid.session_id, "key-b", "node-b", "green", &cipher)
            .await
            .unwrap();
        assert_eq!(migrated.session.logical_node, "node-b");
        assert_eq!(migrated.session.node_url, "https://node-b.test");
        assert_eq!(migrated.session.assigned_ip.as_deref(), Some("10.91.0.2"));
        assert_eq!(migrated.wireguard_endpoint, "node-b-green.test:51820");
        assert_eq!(
            store
                .allocate_address("node-a", "10.90.0.0/24")
                .await
                .unwrap(),
            "10.90.0.2"
        );
    }

    #[tokio::test]
    async fn concurrent_claims_allocate_distinct_addresses() {
        let store = Store::open_in_memory().unwrap();
        let cipher = Arc::new(TokenCipher::new(&[12_u8; 32], 1).unwrap());
        store
            .register_generation(&generation("green"))
            .await
            .unwrap();
        store.promote_generation("node-a", "green").await.unwrap();
        let mut tokens = Vec::new();
        for index in 0..2 {
            let fingerprint = token_lookup_hash(&format!("session-{index}"));
            let intent = store
                .create_payment_intent(
                    None,
                    "node-a",
                    60,
                    fingerprint,
                    1,
                    Utc::now() + chrono::Duration::minutes(5),
                )
                .await
                .unwrap();
            let paid = store
                .redeem_payment(
                    &intent.intent_id,
                    "tempo",
                    &format!("tx-{index}"),
                    fingerprint,
                    3600,
                    &cipher,
                )
                .await
                .unwrap();
            tokens.push(paid.session_id);
        }
        let mut tasks = Vec::new();
        for (index, token) in tokens.into_iter().enumerate() {
            let store = store.clone();
            let cipher = cipher.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .claim_session(&token, &format!("key-{index}"), "node-a", "green", &cipher)
                    .await
                    .unwrap()
                    .session
                    .assigned_ip
                    .unwrap()
            }));
        }
        let mut addresses = Vec::new();
        for task in tasks {
            addresses.push(task.await.unwrap());
        }
        addresses.sort();
        assert_eq!(addresses, ["10.90.0.2", "10.90.0.3"]);
    }

    #[tokio::test]
    async fn stale_lease_accounts_to_cutoff_and_requires_peer_removal() {
        let store = Store::open_in_memory().unwrap();
        let cipher = TokenCipher::new(&[13_u8; 32], 1).unwrap();
        store
            .register_generation(&generation("blue"))
            .await
            .unwrap();
        store.promote_generation("node-a", "blue").await.unwrap();
        let fingerprint = token_lookup_hash("stale-session");
        let intent = store
            .create_payment_intent(
                None,
                "node-a",
                300,
                fingerprint,
                1,
                Utc::now() + chrono::Duration::minutes(5),
            )
            .await
            .unwrap();
        let paid = store
            .redeem_payment(
                &intent.intent_id,
                "tempo",
                "tx-stale",
                fingerprint,
                3600,
                &cipher,
            )
            .await
            .unwrap();
        let claim = store
            .claim_session(&paid.session_id, "client-key", "node-a", "blue", &cipher)
            .await
            .unwrap();
        store
            .acknowledge_peers("node-a", "blue", claim.desired_revision, 1)
            .await
            .unwrap();
        let now = Utc::now();
        {
            let connection = store.connection.lock().await;
            connection
                .execute(
                    "UPDATE sessions SET accounting_at = ?2, last_heartbeat_at = ?2
                     WHERE token_hash = ?1",
                    params![
                        token_lookup_hash(&paid.session_id).to_vec(),
                        (now - chrono::Duration::seconds(100)).to_rfc3339()
                    ],
                )
                .unwrap();
        }
        assert_eq!(store.sweep_stale_sessions(now).await.unwrap(), 1);
        let releasing = store
            .session_status(&paid.session_id, &cipher)
            .await
            .unwrap();
        assert_eq!(releasing.state, SessionState::Paused);
        assert_eq!(releasing.phase.as_deref(), Some("releasing"));
        assert_eq!(releasing.remaining_seconds, 210);
        let snapshot = store
            .peer_snapshot("node-a", "blue", &cipher)
            .await
            .unwrap();
        assert!(snapshot.peers.is_empty());
        store
            .acknowledge_peers("node-a", "blue", snapshot.revision, 0)
            .await
            .unwrap();
        let paused = store
            .session_status(&paid.session_id, &cipher)
            .await
            .unwrap();
        assert_eq!(paused.phase, None);
        assert_eq!(paused.active_generation_id, None);
        assert_eq!(paused.assigned_ip.as_deref(), Some("10.90.0.2"));
    }
}
