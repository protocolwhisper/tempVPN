use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header::WWW_AUTHENTICATE, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use mpp::{
    protocol::{
        core::{Base64UrlJson, PaymentChallenge, PaymentCredential},
        methods::tempo::session::SessionCredentialPayload,
    },
    server::{
        axum::{ChargeChallenger, ChargeConfig, MppCharge},
        SessionChallengeOptions,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    config::Config,
    helpers::{bearer_token, registry_token_matches},
    registry::{NodeAdvertisement, Registry},
    session_v2::{
        stream::{start_metered_stream, MeterOptions},
        StreamingPayments,
    },
    sessions::Sessions,
};
use std::time::Duration;

const VPN_SESSION_PRICE_AMOUNT: &str = "0.01";

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub sessions: Arc<Sessions>,
    pub challenger: Arc<dyn ChargeChallenger>,
    pub registry: Registry,
    pub streaming: Option<Arc<StreamingPayments>>,
}

impl axum::extract::FromRef<AppState> for Arc<dyn ChargeChallenger> {
    fn from_ref(state: &AppState) -> Self {
        state.challenger.clone()
    }
}

struct VpnSessionCharge;

impl ChargeConfig for VpnSessionCharge {
    fn amount() -> &'static str {
        VPN_SESSION_PRICE_AMOUNT
    }

    fn description() -> Option<&'static str> {
        Some("Temporary WireGuard VPN session")
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub duration_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct ConnectSessionRequest {
    pub client_public_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamSessionQuery {
    pub client_public_key: String,
    pub duration_seconds: u64,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    active_sessions: usize,
}

pub fn router(state: AppState) -> Router {
    let streaming_enabled = state.streaming.is_some();
    let mut router = Router::new()
        .route("/health", get(health))
        .route("/nodes", get(nodes))
        .route(
            "/registry/nodes/{node_id}",
            put(register_node).delete(remove_node),
        )
        .route("/sessions", post(create_session))
        .route("/sessions/{session_id}/connect", post(connect_session))
        .route("/sessions/{session_id}/pause", post(pause_session))
        .route("/sessions/{session_id}/heartbeat", post(heartbeat_session))
        .route("/sessions/{session_id}/status", get(session_status))
        .route(
            "/sessions/{session_id}",
            get(get_session).delete(delete_session),
        );
    if streaming_enabled {
        router = router.route(
            "/sessions/stream",
            get(stream_session).head(manage_stream_session),
        );
    }
    router.with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        active_sessions: state.sessions.active_count().await,
    })
}

async fn nodes(State(state): State<AppState>) -> Response {
    if !state.config.registry_mode {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "registry mode is disabled" })),
        )
            .into_response();
    }
    Json(state.registry.active().await).into_response()
}

async fn register_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(node_id): Path<String>,
    Json(node): Json<NodeAdvertisement>,
) -> Response {
    if !state.config.registry_mode {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Err(response) = authorize_registry(&state.config, &headers) {
        return response;
    }
    match state
        .registry
        .upsert(&node_id, node, state.config.registry_lease_seconds)
        .await
    {
        Ok(node) => Json(node).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn remove_node(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(node_id): Path<String>,
) -> Response {
    if !state.config.registry_mode {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Err(response) = authorize_registry(&state.config, &headers) {
        return response;
    }
    if state.registry.remove(&node_id).await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn create_session(
    State(state): State<AppState>,
    _charge: MppCharge<VpnSessionCharge>,
    Json(request): Json<CreateSessionRequest>,
) -> Response {
    match state.sessions.create(request.duration_seconds).await {
        Ok(session) => (StatusCode::CREATED, Json(session)).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn connect_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(request): Json<ConnectSessionRequest>,
) -> Response {
    match state
        .sessions
        .connect(&session_id, request.client_public_key)
        .await
    {
        Ok(session) => Json(session).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn pause_session(State(state): State<AppState>, Path(session_id): Path<String>) -> Response {
    match state.sessions.pause(&session_id).await {
        Ok(Some(session)) => Json(session).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn heartbeat_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Response {
    match state.sessions.heartbeat(&session_id).await {
        Ok(session) => Json(session).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn session_status(State(state): State<AppState>, Path(session_id): Path<String>) -> Response {
    match state.sessions.get(&session_id).await {
        Some(session) => Json(session).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
    }
}

async fn stream_session(
    State(state): State<AppState>,
    Query(query): Query<StreamSessionQuery>,
    headers: HeaderMap,
) -> Response {
    let (streaming, scoped) = match scoped_streaming(&state, &query) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let credential = match payment_credential(&headers) {
        Ok(Some(credential)) => credential,
        Ok(None) => return payment_required(&state, &query, &scoped, None),
        Err(error) => return payment_required(&state, &query, &scoped, Some(error)),
    };
    let verified = match scoped.verify_session(&credential).await {
        Ok(verified) => verified,
        Err(error) => return payment_required(&state, &query, &scoped, Some(error.to_string())),
    };
    let payload: SessionCredentialPayload = match credential.payload_as() {
        Ok(payload) => payload,
        Err(error) => return payment_required(&state, &query, &scoped, Some(error.to_string())),
    };
    let channel_id = payload_channel_id(&payload).to_owned();
    if matches!(payload, SessionCredentialPayload::Close { .. }) {
        terminate_channel_session(&state, &channel_id).await;
        return receipt_response(StatusCode::NO_CONTENT, &verified.receipt, Body::empty());
    }

    let started = match start_metered_stream(
        streaming.store.clone(),
        state.sessions.clone(),
        MeterOptions {
            challenge_id: credential.challenge.id.clone(),
            channel_id,
            client_public_key: query.client_public_key.clone(),
            duration_seconds: query.duration_seconds,
            tick_cost: state.config.streaming.unit_amount,
            billing_interval: Duration::from_secs(state.config.streaming.billing_interval_seconds),
            grace_period: Duration::from_secs(state.config.streaming.grace_period_seconds),
        },
    )
    .await
    {
        Ok(started) => started,
        Err(error) => return payment_required(&state, &query, &scoped, Some(error.to_string())),
    };

    let session_header = HeaderValue::from_str(&started.session.session_id).ok();
    let mut response = receipt_response(
        StatusCode::OK,
        &verified.receipt,
        Body::from_stream(started.body),
    );
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    response.headers_mut().insert(
        axum::http::header::CONNECTION,
        HeaderValue::from_static("keep-alive"),
    );
    if let Some(session_header) = session_header {
        response
            .headers_mut()
            .insert("x-vpn-session-id", session_header);
    }
    response
}

async fn manage_stream_session(
    State(state): State<AppState>,
    Query(query): Query<StreamSessionQuery>,
    headers: HeaderMap,
) -> Response {
    let (_streaming, scoped) = match scoped_streaming(&state, &query) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let credential = match payment_credential(&headers) {
        Ok(Some(credential)) => credential,
        Ok(None) => return payment_required(&state, &query, &scoped, None),
        Err(error) => return payment_required(&state, &query, &scoped, Some(error)),
    };
    let verified = match scoped.verify_session(&credential).await {
        Ok(verified) => verified,
        Err(error) => return payment_required(&state, &query, &scoped, Some(error.to_string())),
    };
    if let Ok(SessionCredentialPayload::Close { channel_id, .. }) = credential.payload_as() {
        terminate_channel_session(&state, &channel_id).await;
    }
    receipt_response(StatusCode::NO_CONTENT, &verified.receipt, Body::empty())
}

#[allow(clippy::result_large_err)]
fn scoped_streaming(
    state: &AppState,
    query: &StreamSessionQuery,
) -> Result<(Arc<StreamingPayments>, crate::session_v2::StreamingMpp), Response> {
    if query.client_public_key.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "client_public_key is required" })),
        )
            .into_response());
    }
    if query.duration_seconds == 0 || query.duration_seconds > state.config.max_duration_seconds {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!(
                    "duration_seconds must be between 1 and {}",
                    state.config.max_duration_seconds
                )
            })),
        )
            .into_response());
    }
    let streaming = state.streaming.clone().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "streaming payments are disabled" })),
        )
            .into_response()
    })?;
    let opaque = Base64UrlJson::from_value(&json!({
        "clientPublicKey": query.client_public_key,
        "durationSeconds": query.duration_seconds.to_string(),
    }))
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response()
    })?;
    let scoped = streaming.mpp.clone().with_opaque(opaque);
    Ok((streaming, scoped))
}

fn payment_credential(headers: &HeaderMap) -> Result<Option<PaymentCredential>, String> {
    let Some(header) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Ok(None);
    };
    let header = header
        .to_str()
        .map_err(|_| "Authorization header is not valid ASCII".to_string())?;
    PaymentCredential::from_header(header)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn session_challenge(
    state: &AppState,
    scoped: &crate::session_v2::StreamingMpp,
) -> Result<PaymentChallenge, String> {
    let amount = state.config.streaming.unit_amount.to_string();
    let suggested = state.config.streaming.suggested_reserve.to_string();
    let description = format!(
        "Metered WireGuard VPN access per {}-second billing interval",
        state.config.streaming.billing_interval_seconds
    );
    scoped
        .session_challenge_with_details(
            &amount,
            &state.config.mpp_payment_currency,
            &state.config.mpp_payment_recipient,
            SessionChallengeOptions {
                unit_type: Some("billing-interval"),
                suggested_deposit: Some(&suggested),
                fee_payer: false,
                description: Some(&description),
                expires: None,
            },
        )
        .map_err(|error| error.to_string())
}

fn payment_required(
    state: &AppState,
    _query: &StreamSessionQuery,
    scoped: &crate::session_v2::StreamingMpp,
    error: Option<String>,
) -> Response {
    let challenge = match session_challenge(state, scoped) {
        Ok(challenge) => challenge,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error })),
            )
                .into_response()
        }
    };
    let header = match challenge.to_header() {
        Ok(header) => header,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response()
        }
    };
    let mut response = (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({
            "error": error.unwrap_or_else(|| "payment required".into()),
            "challenge": challenge,
        })),
    )
        .into_response();
    if let Ok(header) = HeaderValue::from_str(&header) {
        response.headers_mut().insert(WWW_AUTHENTICATE, header);
    }
    response
}

fn receipt_response(status: StatusCode, receipt: &mpp::Receipt, body: Body) -> Response {
    let mut response = Response::builder().status(status).body(body).unwrap();
    if let Ok(value) = receipt.to_header().and_then(|header| {
        HeaderValue::from_str(&header)
            .map_err(|error| mpp::MppError::InvalidConfig(error.to_string()))
    }) {
        response.headers_mut().insert("payment-receipt", value);
    }
    response
}

fn payload_channel_id(payload: &SessionCredentialPayload) -> &str {
    match payload {
        SessionCredentialPayload::Open { channel_id, .. }
        | SessionCredentialPayload::TopUp { channel_id, .. }
        | SessionCredentialPayload::Voucher { channel_id, .. }
        | SessionCredentialPayload::Close { channel_id, .. } => channel_id,
    }
}

async fn terminate_channel_session(state: &AppState, channel_id: &str) {
    let Some(streaming) = &state.streaming else {
        return;
    };
    let Ok(Some(stored)) = streaming.store.get_stored(channel_id).await else {
        return;
    };
    if let Some(lease) = stored.lease {
        let _ = state.sessions.remove(&lease.logical_session_id).await;
        let _ = streaming
            .store
            .release_lease(channel_id, &lease.owner_id)
            .await;
    }
}

async fn get_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    if let Err(response) = authorize(&state.config, &headers) {
        return response;
    }

    match state.sessions.get(&session_id).await {
        Some(session) => Json(session).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
    }
}

async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    if let Err(response) = authorize(&state.config, &headers) {
        return response;
    }

    match state.sessions.remove(&session_id).await {
        Ok(Some(session)) => Json(json!({
            "revoked": true,
            "session_id": session.session_id
        }))
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[allow(clippy::result_large_err)]
fn authorize(config: &Config, headers: &HeaderMap) -> Result<(), Response> {
    let bearer = bearer_token(headers);
    let token = bearer.or_else(|| {
        headers
            .get("x-admin-token")
            .and_then(|value| value.to_str().ok())
    });

    if token == Some(config.admin_token.as_str()) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing or invalid admin token" })),
        )
            .into_response())
    }
}

#[allow(clippy::result_large_err)]
fn authorize_registry(config: &Config, headers: &HeaderMap) -> Result<(), Response> {
    if registry_token_matches(config.registry_token.as_deref(), bearer_token(headers)) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing or invalid registry token" })),
        )
            .into_response())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::Mutex,
    };

    use alloy::{
        primitives::{Address, B256},
        signers::local::PrivateKeySigner,
    };
    use axum::http::{Method, Request};
    use mpp::{
        protocol::{
            core::{format_authorization, parse_www_authenticate},
            intents::SessionRequest,
            methods::tempo::{
                compute_precompile_channel_id_with_escrow, session::ChannelDescriptor,
                sign_precompile_voucher_with_escrow,
            },
            traits::VerificationError,
        },
        server::{tempo, Mpp, TempoConfig},
    };
    use tower::ServiceExt;

    use super::*;

    use crate::{
        config::{ChannelStoreConfig, StreamingConfig, StreamingMode},
        session_v2::{
            chain::{
                ChainFuture, ChainResult, CloseOperation, OpenOperation, ReserveChain,
                ReserveState, TopUpOperation,
            },
            method::{SessionV2Config, TempoSessionV2Method},
            store::SessionStore,
            StreamingPayments,
        },
    };

    #[derive(Default)]
    struct RouteChain {
        state: Mutex<Option<ReserveState>>,
    }

    impl ReserveChain for RouteChain {
        fn open(&self, _operation: OpenOperation) -> ChainFuture<ChainResult> {
            let state = self.state.lock().unwrap().clone().unwrap();
            Box::pin(async move {
                Ok(ChainResult {
                    state,
                    tx_hash: "0xopen".into(),
                })
            })
        }

        fn top_up(&self, _operation: TopUpOperation) -> ChainFuture<ChainResult> {
            Box::pin(async {
                Err(VerificationError::transaction_failed(
                    "top-up not used in route test",
                ))
            })
        }

        fn read(
            &self,
            _channel_id: B256,
            _descriptor: ChannelDescriptor,
        ) -> ChainFuture<ReserveState> {
            let state = self.state.lock().unwrap().clone().unwrap();
            Box::pin(async move { Ok(state) })
        }

        fn close(&self, _operation: CloseOperation) -> ChainFuture<ChainResult> {
            Box::pin(async {
                Err(VerificationError::transaction_failed(
                    "close not used in route test",
                ))
            })
        }
    }

    fn test_config() -> Config {
        Config {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
            admin_token: "admin".into(),
            node_id: "test".into(),
            node_name: "Test Node".into(),
            node_region: "local".into(),
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
            streaming: StreamingConfig {
                enabled: true,
                mode: StreamingMode::Development,
                chain_id: 42_431,
                reserve: "0x4d50500000000000000000000000000000000000"
                    .parse()
                    .unwrap(),
                operator: Address::repeat_byte(0x33),
                unit_amount: 1_000,
                billing_interval_seconds: 60,
                suggested_reserve: 10_000,
                min_voucher_delta: 500,
                grace_period_seconds: 30,
                close_signer: None,
                store: ChannelStoreConfig::Memory,
            },
        }
    }

    async fn state() -> AppState {
        let config = test_config();
        let chain = Arc::new(RouteChain {
            state: Mutex::new(Some(ReserveState {
                deposit: 10_000,
                settled: 0,
                close_requested_at: 0,
                finalized: false,
            })),
        });
        let store = SessionStore::open(&ChannelStoreConfig::Memory)
            .await
            .unwrap();
        let method = TempoSessionV2Method::new(
            chain,
            store.clone(),
            SessionV2Config {
                reserve: config.streaming.reserve,
                chain_id: config.streaming.chain_id,
                operator: config.streaming.operator,
                payee: config.mpp_payment_recipient.parse().unwrap(),
                token: config.mpp_payment_currency.parse().unwrap(),
                unit_amount: config.streaming.unit_amount,
                min_voucher_delta: config.streaming.min_voucher_delta,
            },
        );
        let builder = || {
            tempo(TempoConfig {
                recipient: config.mpp_payment_recipient.as_str(),
            })
            .currency(config.mpp_payment_currency.as_str())
            .rpc_url(config.mpp_rpc_url.as_str())
            .chain_id(config.streaming.chain_id)
            .realm(config.mpp_realm.as_str())
            .secret_key("route-test-secret")
        };
        let challenger = Arc::new(Mpp::create(builder()).unwrap()) as Arc<dyn ChargeChallenger>;
        let streaming = Mpp::create(builder()).unwrap().with_session_method(method);
        let sessions = Sessions::new(&config).unwrap();
        AppState {
            config,
            sessions,
            challenger,
            registry: Registry::default(),
            streaming: Some(Arc::new(StreamingPayments {
                mpp: streaming,
                store,
            })),
        }
    }

    #[tokio::test]
    async fn unpaid_stream_route_advertises_tip1034_v2_and_binds_request() {
        let response = router(state().await)
            .oneshot(
                Request::builder()
                    .uri("/sessions/stream?client_public_key=client-key&duration_seconds=300")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let header = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .unwrap()
            .to_str()
            .unwrap();
        let challenge = parse_www_authenticate(header).unwrap();
        let request: SessionRequest = challenge.request.decode().unwrap();
        assert_eq!(challenge.method.as_str(), "tempo");
        assert_eq!(challenge.intent.as_str(), "session");
        assert_eq!(request.method_details.unwrap()["sessionProtocol"], "v2");
        assert_eq!(request.suggested_deposit.as_deref(), Some("10000"));
        let opaque: serde_json::Value = challenge.opaque.unwrap().decode().unwrap();
        assert_eq!(opaque["clientPublicKey"], "client-key");
        assert_eq!(opaque["durationSeconds"], "300");
    }

    #[tokio::test]
    async fn head_open_is_a_management_action_and_does_not_create_a_peer() {
        let state = state().await;
        let query = StreamSessionQuery {
            client_public_key: "client-key".into(),
            duration_seconds: 300,
        };
        let (_, scoped) = scoped_streaming(&state, &query).ok().unwrap();
        let challenge = session_challenge(&state, &scoped).unwrap();
        let signer = PrivateKeySigner::random();
        let descriptor = ChannelDescriptor {
            payer: signer.address().to_string(),
            payee: state.config.mpp_payment_recipient.clone(),
            operator: state.config.streaming.operator.to_string(),
            token: state.config.mpp_payment_currency.clone(),
            salt: B256::repeat_byte(0x55).to_string(),
            authorized_signer: signer.address().to_string(),
            expiring_nonce_hash: B256::repeat_byte(0x77).to_string(),
        };
        let channel_id = compute_precompile_channel_id_with_escrow(
            signer.address(),
            state.config.mpp_payment_recipient.parse().unwrap(),
            state.config.streaming.operator,
            state.config.mpp_payment_currency.parse().unwrap(),
            B256::repeat_byte(0x55),
            signer.address(),
            B256::repeat_byte(0x77),
            state.config.streaming.reserve,
            state.config.streaming.chain_id,
        );
        let signature = sign_precompile_voucher_with_escrow(
            &signer,
            channel_id,
            2_000,
            state.config.streaming.reserve,
            state.config.streaming.chain_id,
        )
        .await
        .unwrap();
        let credential = PaymentCredential::with_source(
            challenge.to_echo(),
            PaymentCredential::evm_did(
                state.config.streaming.chain_id,
                &signer.address().to_string(),
            ),
            SessionCredentialPayload::Open {
                payload_type: "transaction".into(),
                channel_id: channel_id.to_string(),
                transaction: "0x76".into(),
                descriptor: Some(descriptor),
                authorized_signer: Some(signer.address().to_string()),
                cumulative_amount: "2000".into(),
                signature: alloy::hex::encode_prefixed(signature),
            },
        );
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri("/sessions/stream?client_public_key=client-key&duration_seconds=300")
                    .header(
                        axum::http::header::AUTHORIZATION,
                        format_authorization(&credential).unwrap(),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(response.headers().contains_key("payment-receipt"));
        assert_eq!(state.sessions.active_count().await, 0);
    }

    #[tokio::test]
    async fn invalid_stream_credential_fails_closed_and_one_time_route_remains() {
        let state = state().await;
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/sessions/stream?client_public_key=client-key&duration_seconds=300")
                    .header(axum::http::header::AUTHORIZATION, "Payment invalid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        assert_eq!(state.sessions.active_count().await, 0);

        let response = router(state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sessions")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"client_public_key":"client-key","duration_seconds":300}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        let challenge = parse_www_authenticate(
            response
                .headers()
                .get(WWW_AUTHENTICATE)
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(challenge.intent.as_str(), "charge");
    }
}
