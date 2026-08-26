use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use chrono::Utc;
use futures_core::Stream;
use mpp::{
    protocol::methods::tempo::{
        session_method::{deduct_from_channel, ChannelStore},
        session_receipt::SessionReceipt,
    },
    server::sse::{
        format_message_event, format_need_voucher_event, format_receipt_event, NeedVoucherEvent,
    },
};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    sessions::{Session, Sessions},
};

use super::store::{SessionStore, StreamLease};

#[derive(Debug, Clone)]
pub struct MeterOptions {
    pub challenge_id: String,
    pub channel_id: String,
    pub client_public_key: String,
    pub duration_seconds: u64,
    pub tick_cost: u128,
    pub billing_interval: Duration,
    pub grace_period: Duration,
}

/// SSE body stream that synchronously notifies the metering task when Axum
/// drops the response, allowing the WireGuard peer to be paused immediately.
pub struct MeteredBodyStream {
    receiver: mpsc::Receiver<String>,
    cancel: watch::Sender<bool>,
}

impl Stream for MeteredBodyStream {
    type Item = std::result::Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver
            .poll_recv(cx)
            .map(|item| item.map(|event| Ok(Bytes::from(event))))
    }
}

impl Drop for MeteredBodyStream {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
    }
}

pub struct StartedStream {
    pub session: Session,
    pub body: MeteredBodyStream,
}

pub async fn start_metered_stream(
    store: Arc<SessionStore>,
    sessions: Arc<Sessions>,
    options: MeterOptions,
) -> Result<StartedStream> {
    let stored = store
        .get_stored(&options.channel_id)
        .await
        .map_err(mpp_error)?
        .ok_or_else(|| Error::Mpp("verified payment channel is missing".into()))?;
    if stored.accounting.finalized || stored.accounting.closing {
        return Err(Error::Mpp("payment channel is closed".into()));
    }
    if stored
        .accounting
        .highest_voucher_amount
        .saturating_sub(stored.accounting.spent)
        < options.tick_cost
    {
        return Err(Error::Mpp(
            "accepted voucher does not fund one billing interval".into(),
        ));
    }

    let owner_id = format!("stream_{}", Uuid::new_v4().simple());
    let now = unix_now();
    let lease_ttl = options
        .grace_period
        .max(options.billing_interval.saturating_mul(2));
    let lease_expires = now.saturating_add(duration_secs_i64(lease_ttl));
    let stale_other_session = stored.lease.as_ref().and_then(|previous| {
        (previous.is_expired_at(now) && previous.client_public_key != options.client_public_key)
            .then(|| previous.logical_session_id.clone())
    });
    let existing_session = stored.lease.as_ref().and_then(|lease| {
        (lease.client_public_key == options.client_public_key && lease.is_expired_at(now))
            .then(|| lease.logical_session_id.clone())
    });
    let provisional_session_id = existing_session
        .clone()
        .unwrap_or_else(|| format!("pending_{}", Uuid::new_v4().simple()));

    store
        .acquire_lease(
            &options.channel_id,
            StreamLease {
                owner_id: owner_id.clone(),
                logical_session_id: provisional_session_id,
                client_public_key: options.client_public_key.clone(),
                expires_at_unix: lease_expires,
            },
            now,
        )
        .await
        .map_err(mpp_error)?;
    if let Some(session_id) = stale_other_session {
        if let Err(error) = sessions.remove(&session_id).await {
            let _ = store.release_lease(&options.channel_id, &owner_id).await;
            return Err(error);
        }
    }

    let session = if let Some(session_id) = existing_session {
        match sessions.resume_for_stream(&session_id).await {
            Ok(Some(session)) => session,
            Ok(None) => {
                create_new_session(&store, &sessions, &options, &owner_id, lease_expires).await?
            }
            Err(error) => {
                let _ = store.release_lease(&options.channel_id, &owner_id).await;
                return Err(error);
            }
        }
    } else {
        create_new_session(&store, &sessions, &options, &owner_id, lease_expires).await?
    };

    let (sender, receiver) = mpsc::channel(8);
    let (cancel, cancel_rx) = watch::channel(false);
    tokio::spawn(run_meter(
        store,
        sessions,
        options,
        owner_id,
        session.clone(),
        lease_ttl,
        sender,
        cancel_rx,
    ));

    Ok(StartedStream {
        session,
        body: MeteredBodyStream { receiver, cancel },
    })
}

async fn create_new_session(
    store: &Arc<SessionStore>,
    sessions: &Arc<Sessions>,
    options: &MeterOptions,
    owner_id: &str,
    lease_expires: i64,
) -> Result<Session> {
    let session = match sessions.create_ephemeral(options.duration_seconds).await {
        Ok(session) => match sessions
            .connect(&session.session_id, options.client_public_key.clone())
            .await
        {
            Ok(session) => session,
            Err(error) => {
                let _ = sessions.remove(&session.session_id).await;
                let _ = store.release_lease(&options.channel_id, owner_id).await;
                return Err(error);
            }
        },
        Err(error) => {
            let _ = store.release_lease(&options.channel_id, owner_id).await;
            return Err(error);
        }
    };
    if let Err(error) = store
        .acquire_lease(
            &options.channel_id,
            StreamLease {
                owner_id: owner_id.to_owned(),
                logical_session_id: session.session_id.clone(),
                client_public_key: options.client_public_key.clone(),
                expires_at_unix: lease_expires,
            },
            unix_now(),
        )
        .await
    {
        let _ = sessions.remove(&session.session_id).await;
        let _ = store.release_lease(&options.channel_id, owner_id).await;
        return Err(mpp_error(error));
    }
    Ok(session)
}

#[allow(clippy::too_many_arguments)]
async fn run_meter(
    store: Arc<SessionStore>,
    sessions: Arc<Sessions>,
    options: MeterOptions,
    owner_id: String,
    session: Session,
    lease_ttl: Duration,
    sender: mpsc::Sender<String>,
    mut cancel: watch::Receiver<bool>,
) {
    let connection = serde_json::json!({
        "type": "vpn-session",
        "session": session,
        "channelId": options.channel_id,
        "billingIntervalSeconds": options.billing_interval.as_secs(),
        "unitAmount": options.tick_cost.to_string(),
    });
    if sender
        .send(format_message_event(&connection.to_string()))
        .await
        .is_err()
    {
        disconnect_cleanup(
            store,
            sessions,
            &options.channel_id,
            &owner_id,
            &session.session_id,
            options.grace_period,
        )
        .await;
        return;
    }

    let safety_deadline =
        tokio::time::Instant::now() + Duration::from_secs(session.remaining_seconds);
    let mut next_tick = tokio::time::Instant::now() + options.billing_interval;

    loop {
        let sleep_tick = tokio::time::sleep_until(next_tick);
        let sleep_safety = tokio::time::sleep_until(safety_deadline);
        tokio::pin!(sleep_tick, sleep_safety);
        tokio::select! {
            _ = cancel.changed() => {
                disconnect_cleanup(
                    store,
                    sessions,
                    &options.channel_id,
                    &owner_id,
                    &session.session_id,
                    options.grace_period,
                ).await;
                return;
            }
            _ = &mut sleep_safety => {
                finish_stream(&store, &sessions, &options, &owner_id, &session, &sender).await;
                return;
            }
            _ = store.wait_for_update(&options.channel_id) => {
                match store.get_channel(&options.channel_id).await {
                    Ok(Some(state)) if state.finalized || state.closing => {
                        finish_stream(&store, &sessions, &options, &owner_id, &session, &sender).await;
                        return;
                    }
                    _ => continue,
                }
            }
            _ = &mut sleep_tick => {}
        }

        match deduct_from_channel(&*store, &options.channel_id, options.tick_cost).await {
            Ok(state) => {
                let lease = StreamLease {
                    owner_id: owner_id.clone(),
                    logical_session_id: session.session_id.clone(),
                    client_public_key: options.client_public_key.clone(),
                    expires_at_unix: unix_now().saturating_add(duration_secs_i64(lease_ttl)),
                };
                if store
                    .acquire_lease(&options.channel_id, lease, unix_now())
                    .await
                    .is_err()
                {
                    finish_stream(&store, &sessions, &options, &owner_id, &session, &sender).await;
                    return;
                }
                let paid = serde_json::json!({
                    "type": "paid-interval",
                    "sessionId": session.session_id,
                    "channelId": options.channel_id,
                    "units": state.units,
                    "spent": state.spent.to_string(),
                });
                if sender
                    .send(format_message_event(&paid.to_string()))
                    .await
                    .is_err()
                {
                    disconnect_cleanup(
                        store,
                        sessions,
                        &options.channel_id,
                        &owner_id,
                        &session.session_id,
                        options.grace_period,
                    )
                    .await;
                    return;
                }
                next_tick = tokio::time::Instant::now() + options.billing_interval;
            }
            Err(_) => {
                let _ = sessions.pause_for_stream(&session.session_id).await;
                let grace_deadline = tokio::time::Instant::now() + options.grace_period;
                if let Ok(Some(state)) = store.get_channel(&options.channel_id).await {
                    if state.finalized || state.closing {
                        finish_stream(&store, &sessions, &options, &owner_id, &session, &sender)
                            .await;
                        return;
                    }
                    let need = NeedVoucherEvent {
                        channel_id: options.channel_id.clone(),
                        required_cumulative: state
                            .spent
                            .saturating_add(options.tick_cost)
                            .to_string(),
                        accepted_cumulative: state.highest_voucher_amount.to_string(),
                        deposit: state.deposit.to_string(),
                    };
                    if sender.send(format_need_voucher_event(&need)).await.is_err() {
                        disconnect_cleanup(
                            store,
                            sessions,
                            &options.channel_id,
                            &owner_id,
                            &session.session_id,
                            options.grace_period,
                        )
                        .await;
                        return;
                    }
                }

                loop {
                    let poll = tokio::time::sleep(Duration::from_millis(100));
                    let grace = tokio::time::sleep_until(grace_deadline);
                    tokio::pin!(poll, grace);
                    tokio::select! {
                        _ = cancel.changed() => {
                            disconnect_cleanup(
                                store,
                                sessions,
                                &options.channel_id,
                                &owner_id,
                                &session.session_id,
                                options.grace_period,
                            ).await;
                            return;
                        }
                        _ = &mut grace => {
                            finish_stream(&store, &sessions, &options, &owner_id, &session, &sender).await;
                            return;
                        }
                        _ = store.wait_for_update(&options.channel_id) => {}
                        _ = &mut poll => {}
                    }
                    match store.get_channel(&options.channel_id).await {
                        Ok(Some(state)) if state.finalized || state.closing => {
                            finish_stream(
                                &store, &sessions, &options, &owner_id, &session, &sender,
                            )
                            .await;
                            return;
                        }
                        Ok(Some(state))
                            if state.highest_voucher_amount.saturating_sub(state.spent)
                                >= options.tick_cost =>
                        {
                            if sessions
                                .resume_for_stream(&session.session_id)
                                .await
                                .is_err()
                            {
                                finish_stream(
                                    &store, &sessions, &options, &owner_id, &session, &sender,
                                )
                                .await;
                                return;
                            }
                            next_tick = tokio::time::Instant::now() + options.billing_interval;
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

async fn finish_stream(
    store: &Arc<SessionStore>,
    sessions: &Arc<Sessions>,
    options: &MeterOptions,
    owner_id: &str,
    session: &Session,
    sender: &mpsc::Sender<String>,
) {
    if let Ok(Some(state)) = store.get_channel(&options.channel_id).await {
        let mut receipt = SessionReceipt::new(
            Utc::now().to_rfc3339(),
            &options.challenge_id,
            &options.channel_id,
            state.highest_voucher_amount.to_string(),
            state.spent.to_string(),
        );
        receipt.units = Some(state.units);
        let _ = sender.send(format_receipt_event(&receipt)).await;
    }
    let _ = sessions.remove(&session.session_id).await;
    let _ = store.release_lease(&options.channel_id, owner_id).await;
}

async fn disconnect_cleanup(
    store: Arc<SessionStore>,
    sessions: Arc<Sessions>,
    channel_id: &str,
    owner_id: &str,
    session_id: &str,
    grace_period: Duration,
) {
    let _ = sessions.pause_for_stream(session_id).await;
    let _ = store.expire_lease(channel_id, owner_id, unix_now()).await;
    let channel_id = channel_id.to_owned();
    let owner_id = owner_id.to_owned();
    let session_id = session_id.to_owned();
    tokio::spawn(async move {
        tokio::time::sleep(grace_period).await;
        let remove = store
            .get_stored(&channel_id)
            .await
            .ok()
            .flatten()
            .and_then(|row| row.lease)
            .is_some_and(|lease| {
                lease.owner_id == owner_id
                    && lease.logical_session_id == session_id
                    && lease.is_expired_at(unix_now())
            });
        if remove {
            let _ = sessions.remove(&session_id).await;
            let _ = store.release_lease(&channel_id, &owner_id).await;
        }
    });
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn duration_secs_i64(duration: Duration) -> i64 {
    duration.as_secs().min(i64::MAX as u64) as i64
}

fn mpp_error(error: impl std::fmt::Display) -> Error {
    Error::Mpp(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use alloy::primitives::{Address, B256};
    use futures_util::StreamExt;
    use mpp::protocol::methods::tempo::{
        session::ChannelDescriptor,
        session_method::{ChannelState, ChannelStore},
    };

    use crate::config::{ChannelStoreConfig, Config, StreamingConfig, StreamingMode};

    use super::*;

    fn config() -> Config {
        Config {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
            admin_token: "admin".into(),
            node_id: "test".into(),
            node_name: "Test Node".into(),
            node_region: "local".into(),
            node_country_code: Some("DE".into()),
            node_subdivision_code: None,
            node_city: None,
            accepting_sessions: true,
            public_api_url: "http://127.0.0.1:8080".into(),
            expected_exit_ip: "127.0.0.1".into(),
            registry_mode: false,
            registry_url: None,
            registry_token: None,
            registry_refresh_seconds: 30,
            registry_lease_seconds: 90,
            wg_interface: "wg0".into(),
            wg_command: "wg".into(),
            server_public_key: "server-key".into(),
            endpoint: "vpn.test:51820".into(),
            tunnel_cidr: "10.8.0.0/24".into(),
            max_duration_seconds: 3600,
            grace_period_seconds: 604_800,
            stale_timeout_seconds: 90,
            sweep_interval_seconds: 10,
            cleanup_on_shutdown: true,
            mock_wg: true,
            mpp_rpc_url: "https://rpc.moderato.tempo.xyz".into(),
            mpp_realm: "vpn.test".into(),
            mpp_payment_currency: Address::repeat_byte(0x44).to_string(),
            mpp_payment_recipient: Address::repeat_byte(0x22).to_string(),
            node_state_store: crate::config::NodeStateStoreConfig::Memory,
            audit_log_path: None,
            coordinator: None,
            streaming: StreamingConfig {
                enabled: true,
                mode: StreamingMode::Development,
                chain_id: 42_431,
                reserve: Address::repeat_byte(0x4d),
                operator: Address::repeat_byte(0x33),
                unit_amount: 1_000,
                billing_interval_seconds: 1,
                suggested_reserve: 10_000,
                min_voucher_delta: 500,
                grace_period_seconds: 1,
                close_signer: None,
                store: ChannelStoreConfig::Memory,
            },
        }
    }

    fn descriptor() -> ChannelDescriptor {
        ChannelDescriptor {
            payer: Address::repeat_byte(0x11).to_string(),
            payee: Address::repeat_byte(0x22).to_string(),
            operator: Address::repeat_byte(0x33).to_string(),
            token: Address::repeat_byte(0x44).to_string(),
            salt: B256::repeat_byte(0x55).to_string(),
            authorized_signer: Address::repeat_byte(0x66).to_string(),
            expiring_nonce_hash: B256::repeat_byte(0x77).to_string(),
        }
    }

    fn channel(channel_id: &str, accepted: u128) -> ChannelState {
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
            highest_voucher_amount: accepted,
            highest_voucher_signature: None,
            spent: 0,
            units: 0,
            finalized: false,
            closing: false,
            close_requested_at: 0,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    async fn event(stream: &mut MeteredBodyStream) -> String {
        String::from_utf8(
            stream
                .next()
                .await
                .expect("stream event")
                .expect("infallible")
                .to_vec(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn exhaustion_pauses_and_a_new_voucher_resumes_without_charging_pause() {
        let store = SessionStore::open(&ChannelStoreConfig::Memory)
            .await
            .unwrap();
        store
            .upsert_verified(channel("0xmeter", 1_000), descriptor())
            .await
            .unwrap();
        let sessions = Sessions::new(&config()).unwrap();
        let started = start_metered_stream(
            store.clone(),
            sessions.clone(),
            MeterOptions {
                challenge_id: "challenge".into(),
                channel_id: "0xmeter".into(),
                client_public_key: "client-key".into(),
                duration_seconds: 5,
                tick_cost: 1_000,
                billing_interval: Duration::from_millis(20),
                grace_period: Duration::from_millis(250),
            },
        )
        .await
        .unwrap();
        let session_id = started.session.session_id.clone();
        let mut body = started.body;
        assert!(event(&mut body).await.contains("vpn-session"));
        assert!(event(&mut body).await.contains("paid-interval"));
        assert!(event(&mut body).await.contains("payment-need-voucher"));
        assert!(!sessions.is_active(&session_id).await);

        store
            .update_channel(
                "0xmeter",
                Box::new(|current| {
                    Ok(current.map(|current| ChannelState {
                        highest_voucher_amount: 2_000,
                        ..current
                    }))
                }),
            )
            .await
            .unwrap();
        assert!(event(&mut body).await.contains("paid-interval"));
        assert!(sessions.is_active(&session_id).await);
        assert_eq!(
            store.get_channel("0xmeter").await.unwrap().unwrap().units,
            2
        );

        drop(body);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!sessions.is_active(&session_id).await);
        tokio::time::sleep(Duration::from_millis(280)).await;
        assert!(sessions.get(&session_id).await.is_none());
    }

    #[tokio::test]
    async fn finalized_channel_emits_receipt_and_removes_peer() {
        let store = SessionStore::open(&ChannelStoreConfig::Memory)
            .await
            .unwrap();
        store
            .upsert_verified(channel("0xfinal", 2_000), descriptor())
            .await
            .unwrap();
        let sessions = Sessions::new(&config()).unwrap();
        let started = start_metered_stream(
            store.clone(),
            sessions.clone(),
            MeterOptions {
                challenge_id: "challenge".into(),
                channel_id: "0xfinal".into(),
                client_public_key: "client-key".into(),
                duration_seconds: 5,
                tick_cost: 1_000,
                billing_interval: Duration::from_secs(10),
                grace_period: Duration::from_millis(100),
            },
        )
        .await
        .unwrap();
        let session_id = started.session.session_id.clone();
        let mut body = started.body;
        assert!(event(&mut body).await.contains("vpn-session"));
        store
            .update_channel(
                "0xfinal",
                Box::new(|current| {
                    Ok(current.map(|current| ChannelState {
                        finalized: true,
                        ..current
                    }))
                }),
            )
            .await
            .unwrap();
        assert!(event(&mut body).await.contains("payment-receipt"));
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(sessions.get(&session_id).await.is_none());
    }

    #[tokio::test]
    async fn disconnected_stream_reconnects_to_the_same_logical_session() {
        let store = SessionStore::open(&ChannelStoreConfig::Memory)
            .await
            .unwrap();
        store
            .upsert_verified(channel("0xreconnect", 2_000), descriptor())
            .await
            .unwrap();
        let sessions = Sessions::new(&config()).unwrap();
        let options = MeterOptions {
            challenge_id: "challenge".into(),
            channel_id: "0xreconnect".into(),
            client_public_key: "client-key".into(),
            duration_seconds: 5,
            tick_cost: 1_000,
            billing_interval: Duration::from_secs(10),
            grace_period: Duration::from_millis(200),
        };
        let first = start_metered_stream(store.clone(), sessions.clone(), options.clone())
            .await
            .unwrap();
        let session_id = first.session.session_id.clone();
        let mut first_body = first.body;
        assert!(event(&mut first_body).await.contains("vpn-session"));
        drop(first_body);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!sessions.is_active(&session_id).await);

        let second = start_metered_stream(store.clone(), sessions.clone(), options)
            .await
            .unwrap();
        assert_eq!(second.session.session_id, session_id);
        assert!(sessions.is_active(&session_id).await);
        drop(second.body);
        tokio::time::sleep(Duration::from_millis(240)).await;
        assert!(sessions.get(&session_id).await.is_none());
    }
}
