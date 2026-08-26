use std::{collections::BTreeMap, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{OriginalUri, Path, Query, State},
    http::{
        header::{ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, WWW_AUTHENTICATE},
        HeaderMap, HeaderName, HeaderValue, Method, StatusCode,
    },
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use futures::{
    future::{join_all, BoxFuture},
    TryStreamExt,
};
use mpp::{
    protocol::core::{extract_payment_scheme, PaymentCredential},
    server::axum::{ChallengeOptions, ChargeChallenger, PaymentRequired},
};
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tempvpn_coordinator_client::{ControlPlaneClient, Error as CoordinatorError, SessionRecord};
use tower_http::cors::{Any, CorsLayer};

const DEGRADED_HEADER: &str = "x-tempvpn-degraded";
const NODE_ID_HEADER: &str = "x-tempvpn-node-id";
const PAYMENT_RECEIPT_HEADER: &str = "payment-receipt";
const CONTROL_PLANE_TOKEN_HEADER: &str = "x-tempvpn-control-token";
const OPENAPI_DOCUMENT: &str = include_str!("../openapi.json");

#[derive(Clone, Debug)]
pub struct Upstream {
    pub name: String,
    pub url: String,
}

#[derive(Clone)]
pub struct AppState {
    client: reqwest::Client,
    proxy_client: reqwest::Client,
    upstreams: Arc<[Upstream]>,
    fixed_payments: Option<Arc<FixedPaymentState>>,
}

#[derive(Clone, Debug)]
pub struct FixedPaymentSettings {
    pub realm: String,
    pub currency: String,
    pub recipient: String,
    pub max_duration_seconds: u64,
    pub grace_period_seconds: u64,
    pub node_control_token: String,
}

pub trait FixedSessionCoordinator: Send + Sync {
    fn create_payment_intent(
        &self,
        intent_id: String,
        node_id: String,
        duration_seconds: u64,
        fingerprint: [u8; 32],
    ) -> BoxFuture<'static, Result<(), CoordinatorError>>;
    fn redeem_payment(
        &self,
        intent_id: String,
        transaction_reference: String,
        fingerprint: [u8; 32],
        grace_seconds: u64,
    ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>>;
    fn status(
        &self,
        session_id: String,
    ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>>;
    fn heartbeat(
        &self,
        session_id: String,
    ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>>;
    fn pause(
        &self,
        session_id: String,
    ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>>;
}

impl FixedSessionCoordinator for ControlPlaneClient {
    fn create_payment_intent(
        &self,
        intent_id: String,
        node_id: String,
        duration_seconds: u64,
        fingerprint: [u8; 32],
    ) -> BoxFuture<'static, Result<(), CoordinatorError>> {
        let client = self.clone();
        Box::pin(async move {
            client
                .create_payment_intent(intent_id, node_id, duration_seconds, fingerprint, 1)
                .await?;
            Ok(())
        })
    }
    fn redeem_payment(
        &self,
        intent_id: String,
        transaction_reference: String,
        fingerprint: [u8; 32],
        grace_seconds: u64,
    ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>> {
        let client = self.clone();
        Box::pin(async move {
            client
                .redeem_payment(intent_id, transaction_reference, fingerprint, grace_seconds)
                .await
        })
    }
    fn status(
        &self,
        session_id: String,
    ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>> {
        let client = self.clone();
        Box::pin(async move { client.status(session_id).await })
    }
    fn heartbeat(
        &self,
        session_id: String,
    ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>> {
        let client = self.clone();
        Box::pin(async move { client.heartbeat(session_id).await })
    }
    fn pause(
        &self,
        session_id: String,
    ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>> {
        let client = self.clone();
        Box::pin(async move { client.pause(session_id).await })
    }
}

struct FixedPaymentState {
    coordinator: Arc<dyn FixedSessionCoordinator>,
    challenger: Arc<dyn ChargeChallenger>,
    settings: FixedPaymentSettings,
}

impl AppState {
    pub fn new(upstreams: Vec<Upstream>, timeout: Duration) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder().timeout(timeout).build()?,
            // A total request timeout would terminate long-lived SSE response bodies.
            // Bound only the connection setup for requests proxied to nodes.
            proxy_client: reqwest::Client::builder()
                .connect_timeout(timeout)
                .build()?,
            upstreams: upstreams.into(),
            fixed_payments: None,
        })
    }

    pub fn with_fixed_payments(
        mut self,
        coordinator: Arc<dyn FixedSessionCoordinator>,
        challenger: Arc<dyn ChargeChallenger>,
        settings: FixedPaymentSettings,
    ) -> Self {
        self.fixed_payments = Some(Arc::new(FixedPaymentState {
            coordinator,
            challenger,
            settings,
        }));
        self
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NodesQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeRecord {
    pub id: String,
    pub lease_expires_at: DateTime<Utc>,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

#[derive(Debug)]
struct UpstreamResult {
    name: String,
    nodes: Result<Vec<NodeRecord>, ()>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    upstreams: BTreeMap<String, bool>,
}

#[derive(Debug, Deserialize)]
struct CreateFixedSessionRequest {
    node_id: String,
    duration_seconds: u64,
}

pub fn router(state: AppState) -> Router {
    let degraded = HeaderName::from_static(DEGRADED_HEADER);
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::HEAD, Method::POST, Method::OPTIONS])
        .allow_headers(Any)
        .expose_headers([degraded]);

    Router::new()
        .route("/", get(service_root))
        .route("/docs", get(service_docs))
        .route("/docs/", get(service_docs))
        .route("/docs/markdown", get(service_docs_markdown))
        .route("/docs/markdown.md", get(service_docs_markdown))
        .route("/llms.txt", get(llms_txt))
        .route("/health", get(health))
        .route("/nodes", get(nodes))
        .route("/sessions", post(create_session))
        .route("/sessions/{session_id}/connect", post(connect_session))
        .route("/sessions/{session_id}/pause", post(pause_session))
        .route("/sessions/{session_id}/heartbeat", post(heartbeat_session))
        .route("/sessions/{session_id}/status", get(session_status))
        .route(
            "/sessions/stream",
            post(stream_session).head(manage_stream_session),
        )
        .route("/openapi.json", get(openapi))
        .layer(cors)
        .with_state(state)
}

async fn service_root() -> Json<Value> {
    Json(json!({
        "service": "TempVPN global registry",
        "nodes": "/nodes",
        "sessions": "/sessions",
        "health": "/health",
        "docs": "https://tempvpn.xyz/docs/",
        "docs_markdown": "/docs/markdown",
        "openapi": "/openapi.json",
        "llms": "/llms.txt"
    }))
}

async fn service_docs() -> Redirect {
    Redirect::permanent("https://tempvpn.xyz/docs/")
}

async fn service_docs_markdown() -> Response {
    let document = "# TempVPN\n\nTempVPN sells temporary WireGuard VPN access through Tempo MPP on mainnet. The registry is the control-plane origin for discovery, payment, and session lifecycle requests.\n\n## Agent workflow\n\n1. Discover a node with `GET /nodes?available=true` and retain its `id`.\n2. Buy fixed access with `POST /sessions` and JSON `{\"node_id\":\"madrid\",\"duration_seconds\":1800}`.\n3. Connect it with `POST /sessions/{session_id}/connect` and the selected `node_id` plus a locally generated WireGuard public key.\n4. Use `GET /sessions/{session_id}/status`, `POST /sessions/{session_id}/heartbeat`, and `POST /sessions/{session_id}/pause` throughout the fixed session. A paused balance can reconnect through another available node.\n5. Streaming is node-bound: use `POST /sessions/stream` to open and `HEAD /sessions/stream?node_id=<id>&client_public_key=<key>&duration_seconds=<seconds>` for voucher, top-up, resume, or close operations.\n\nFixed sessions cost $0.01 per minute. Requested duration is seconds and must be a positive multiple of 60; the charge is `$0.01 × (duration_seconds / 60)`. Treat the runtime HTTP 402 challenge as authoritative for payment details. Node `api_url` values are diagnostic metadata, not client payment origins.\n\nMachine-readable API: /openapi.json\n";
    ([(CONTENT_TYPE, "text/plain; charset=utf-8")], document).into_response()
}

async fn llms_txt() -> Response {
    let document = "# TempVPN\n\n> Buy temporary WireGuard VPN access with Tempo MPP.\n\nService: https://registry.tempvpn.xyz\nOpenAPI: https://registry.tempvpn.xyz/openapi.json\nDocs: https://tempvpn.xyz/docs/\nMarkdown docs: https://registry.tempvpn.xyz/docs/markdown\n\nUse the registry origin for every request below. Node api_url values are diagnostic metadata, not payment origins.\nDiscover nodes: GET /nodes?available=true; retain the selected node id.\nFixed purchase: POST /sessions JSON {\"node_id\": \"madrid\", \"duration_seconds\": 1800}\nConnect fixed session: POST /sessions/<session_id>/connect JSON {\"node_id\": \"madrid\", \"client_public_key\": \"<wireguard-public-key>\"}\nStatus: GET /sessions/<session_id>/status\nHeartbeat: POST /sessions/<session_id>/heartbeat\nPause: POST /sessions/<session_id>/pause\nStreaming open: POST /sessions/stream JSON {\"node_id\": \"madrid\", \"client_public_key\": \"<wireguard-public-key>\", \"duration_seconds\": 1800}\nStreaming voucher/top-up/resume/close: HEAD /sessions/stream?node_id=madrid&client_public_key=<wireguard-public-key>&duration_seconds=1800\nStreaming is a separate node-bound metered product and is not a portable fixed-session balance.\nFixed price: $0.01 per minute. Requested duration is seconds and must be a positive multiple of 60: `$0.01 × (duration_seconds / 60)`. Payment: Tempo mainnet MPP; follow the runtime 402 challenge for authoritative payment details.\nNever send a wallet private key or WireGuard private key to TempVPN.\n";
    ([(CONTENT_TYPE, "text/plain; charset=utf-8")], document).into_response()
}

async fn openapi() -> Response {
    let mut response = OPENAPI_DOCUMENT.into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

async fn nodes(State(state): State<AppState>, Query(query): Query<NodesQuery>) -> Response {
    let results = fetch_all(&state, &query).await;
    let failed = results
        .iter()
        .filter(|result| result.nodes.is_err())
        .count();
    let successful = results.len().saturating_sub(failed);

    if successful == 0 {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "all registry upstreams are unavailable" })),
        )
            .into_response();
    }

    let merged = merge_nodes(results);

    let mut response = Json(merged.into_values().collect::<Vec<_>>()).into_response();
    response.headers_mut().insert(
        HeaderName::from_static(DEGRADED_HEADER),
        HeaderValue::from_static(if failed > 0 { "true" } else { "false" }),
    );
    response
}

async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(fixed) = &state.fixed_payments else {
        return proxy_to_selected_node(&state, headers, body, Method::POST, "/sessions").await;
    };
    let request = match serde_json::from_slice::<CreateFixedSessionRequest>(&body) {
        Ok(request) => request,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "node_id and duration_seconds are required" })),
            )
                .into_response()
        }
    };
    if request.node_id.trim().is_empty()
        || request.duration_seconds == 0
        || request.duration_seconds % 60 != 0
        || request.duration_seconds > fixed.settings.max_duration_seconds
    {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": format!("duration_seconds must be a positive multiple of 60 no greater than {}", fixed.settings.max_duration_seconds)
        }))).into_response();
    }
    if let Err(response) = require_eligible_node(&state, &request.node_id).await {
        return response;
    }
    let amount = fixed_session_price(request.duration_seconds);
    let fingerprint = fixed_session_fingerprint(
        &request.node_id,
        request.duration_seconds,
        &amount,
        &fixed.settings,
    );
    let payment_header = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(extract_payment_scheme);
    let Some(payment_header) = payment_header else {
        return registry_payment_required(fixed, &request, &amount, fingerprint).await;
    };
    let credential = match PaymentCredential::from_header(payment_header) {
        Ok(credential) => credential,
        Err(_) => return registry_payment_required(fixed, &request, &amount, fingerprint).await,
    };
    let receipt = match fixed
        .challenger
        .verify_payment_for_amount(payment_header, &amount)
        .await
    {
        Ok(receipt) => receipt,
        Err(_) => return registry_payment_required(fixed, &request, &amount, fingerprint).await,
    };
    match fixed
        .coordinator
        .redeem_payment(
            credential.challenge.id,
            receipt.reference.clone(),
            fingerprint,
            fixed.settings.grace_period_seconds,
        )
        .await
    {
        Ok(session) => {
            let mut response = (
                StatusCode::CREATED,
                Json(portable_session_document(&session)),
            )
                .into_response();
            if let Ok(value) = receipt.to_header() {
                if let Ok(header) = HeaderValue::from_str(&value) {
                    response
                        .headers_mut()
                        .insert(HeaderName::from_static(PAYMENT_RECEIPT_HEADER), header);
                }
            }
            response
        }
        Err(error) => coordinator_error_response(error),
    }
}

async fn connect_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    mut headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(fixed) = &state.fixed_payments {
        let Ok(token) = HeaderValue::from_str(&fixed.settings.node_control_token) else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "registry node-control token is invalid" })),
            )
                .into_response();
        };
        headers.insert(HeaderName::from_static(CONTROL_PLANE_TOKEN_HEADER), token);
    }
    proxy_to_selected_node(
        &state,
        headers,
        body,
        Method::POST,
        &format!("/sessions/{session_id}/connect"),
    )
    .await
}

async fn stream_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    proxy_to_selected_node(&state, headers, body, Method::POST, "/sessions/stream").await
}

async fn manage_stream_session(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Response {
    let node_id = headers
        .get(NODE_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| query_parameter(uri.query(), "node_id"));
    let Some(node_id) = node_id else {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "node_id is required in the query or x-tempvpn-node-id header" }),
            ),
        )
            .into_response();
    };
    proxy_to_node_id(
        &state,
        headers,
        Bytes::new(),
        Method::HEAD,
        uri.path_and_query()
            .map(|value| value.as_str())
            .unwrap_or("/sessions/stream"),
        &node_id,
    )
    .await
}

async fn pause_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(fixed) = &state.fixed_payments {
        return match fixed.coordinator.pause(session_id).await {
            Ok(session) => Json(portable_session_document(&session)).into_response(),
            Err(error) => coordinator_error_response(error),
        };
    }
    proxy_to_any_node(
        &state,
        headers,
        Bytes::new(),
        Method::POST,
        &format!("/sessions/{session_id}/pause"),
    )
    .await
}

async fn heartbeat_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(fixed) = &state.fixed_payments {
        return match fixed.coordinator.heartbeat(session_id).await {
            Ok(session) => Json(portable_session_document(&session)).into_response(),
            Err(error) => coordinator_error_response(error),
        };
    }
    proxy_to_any_node(
        &state,
        headers,
        Bytes::new(),
        Method::POST,
        &format!("/sessions/{session_id}/heartbeat"),
    )
    .await
}

async fn session_status(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(fixed) = &state.fixed_payments {
        return match fixed.coordinator.status(session_id).await {
            Ok(session) => Json(portable_session_document(&session)).into_response(),
            Err(error) => coordinator_error_response(error),
        };
    }
    proxy_to_any_node(
        &state,
        headers,
        Bytes::new(),
        Method::GET,
        &format!("/sessions/{session_id}/status"),
    )
    .await
}

async fn require_eligible_node(state: &AppState, node_id: &str) -> Result<(), Response> {
    let nodes = merge_nodes(
        fetch_all(
            state,
            &NodesQuery {
                available: Some(true),
                ..NodesQuery::default()
            },
        )
        .await,
    );
    let Some(node) = nodes.get(node_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "selected node is unavailable" })),
        )
            .into_response());
    };
    let accepting = node
        .fields
        .get("accepting_sessions")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let slots = node
        .fields
        .get("available_slots")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if !accepting || slots == 0 || node.lease_expires_at <= Utc::now() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "selected node is not accepting sessions" })),
        )
            .into_response());
    }
    Ok(())
}

fn fixed_session_price(duration_seconds: u64) -> String {
    let cents = duration_seconds / 60;
    format!("{}.{:02}", cents / 100, cents % 100)
}

fn portable_session_document(session: &SessionRecord) -> Value {
    json!({
        "session_id": session.session_id,
        "node_id": session.logical_node,
        "node_url": session.node_url,
        "state": session.state,
        "phase": session.phase,
        "total_seconds": session.total_seconds,
        "remaining_seconds": session.remaining_seconds,
        "created_at": session.created_at,
        "connected_at": session.connected_at,
        "last_heartbeat_at": session.last_heartbeat_at,
        "not_after": session.grace_deadline,
        "assigned_ip": session.assigned_ip,
        "client_public_key": session.client_public_key,
    })
}

fn fixed_session_fingerprint(
    node_id: &str,
    duration_seconds: u64,
    amount: &str,
    settings: &FixedPaymentSettings,
) -> [u8; 32] {
    let input = format!(
        "fixed-session-v2\0{node_id}\0{duration_seconds}\0{amount}\0{}\0{}\0{}\0/sessions",
        settings.realm, settings.currency, settings.recipient
    );
    digest(&SHA256, input.as_bytes())
        .as_ref()
        .try_into()
        .expect("SHA-256 is 32 bytes")
}

async fn registry_payment_required(
    fixed: &FixedPaymentState,
    request: &CreateFixedSessionRequest,
    amount: &str,
    fingerprint: [u8; 32],
) -> Response {
    let challenge = match fixed.challenger.challenge(
        amount,
        ChallengeOptions {
            description: Some("Temporary WireGuard VPN session"),
            mppx_scope: None,
        },
    ) {
        Ok(challenge) => challenge,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string(), "retryable": true })),
            )
                .into_response()
        }
    };
    match fixed
        .coordinator
        .create_payment_intent(
            challenge.id.clone(),
            request.node_id.clone(),
            request.duration_seconds,
            fingerprint,
        )
        .await
    {
        Ok(()) => PaymentRequired(challenge).into_response(),
        Err(error) => coordinator_error_response(error),
    }
}

fn coordinator_error_response(error: CoordinatorError) -> Response {
    let (status, retryable) = match &error {
        CoordinatorError::Rejected { status: 404, .. } => (StatusCode::NOT_FOUND, false),
        CoordinatorError::Rejected { status: 409, .. } => (StatusCode::CONFLICT, false),
        CoordinatorError::Rejected { status: 400, .. } => (StatusCode::BAD_REQUEST, false),
        CoordinatorError::Unavailable(_)
        | CoordinatorError::Http(_)
        | CoordinatorError::Io(_)
        | CoordinatorError::Configuration(_)
        | CoordinatorError::Protocol(_)
        | CoordinatorError::Rejected { .. } => (StatusCode::SERVICE_UNAVAILABLE, true),
    };
    let mut response = (
        status,
        Json(json!({ "error": error.to_string(), "retryable": retryable })),
    )
        .into_response();
    if retryable {
        response.headers_mut().insert(
            axum::http::header::RETRY_AFTER,
            HeaderValue::from_static("1"),
        );
    }
    response
}

async fn proxy_to_selected_node(
    state: &AppState,
    headers: HeaderMap,
    body: Bytes,
    method: Method,
    path: &str,
) -> Response {
    let node_id = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("node_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            headers
                .get(NODE_ID_HEADER)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        });
    let Some(node_id) = node_id else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "node_id is required" })),
        )
            .into_response();
    };
    proxy_to_node_id(state, headers, body, method, path, &node_id).await
}

async fn proxy_to_node_id(
    state: &AppState,
    headers: HeaderMap,
    body: Bytes,
    method: Method,
    path: &str,
    node_id: &str,
) -> Response {
    let nodes = merge_nodes(fetch_all(state, &NodesQuery::default()).await);
    let Some(node) = nodes.get(node_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "selected node is unavailable" })),
        )
            .into_response();
    };
    let Some(api_url) = node.fields.get("api_url").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "selected node has no API URL" })),
        )
            .into_response();
    };
    proxy_request(state, headers, body, method, api_url, path).await
}

fn query_parameter(query: Option<&str>, wanted: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        (name == wanted && !value.is_empty()).then(|| value.to_owned())
    })
}

async fn proxy_to_any_node(
    state: &AppState,
    headers: HeaderMap,
    body: Bytes,
    method: Method,
    path: &str,
) -> Response {
    let nodes = merge_nodes(fetch_all(state, &NodesQuery::default()).await);
    let Some(api_url) = nodes
        .values()
        .find_map(|node| node.fields.get("api_url").and_then(Value::as_str))
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "no node API is currently available" })),
        )
            .into_response();
    };
    proxy_request(state, headers, body, method, api_url, path).await
}

async fn proxy_request(
    state: &AppState,
    headers: HeaderMap,
    body: Bytes,
    method: Method,
    api_url: &str,
    path: &str,
) -> Response {
    let url = format!("{}{}", api_url.trim_end_matches('/'), path);
    let mut request = state.proxy_client.request(method, url).body(body);
    for name in [
        AUTHORIZATION,
        ACCEPT,
        CONTENT_TYPE,
        HeaderName::from_static(CONTROL_PLANE_TOKEN_HEADER),
    ] {
        if let Some(value) = headers.get(&name) {
            request = request.header(name, value);
        }
    }
    let upstream = match request.send().await {
        Ok(response) => response,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "selected node is unavailable" })),
            )
                .into_response()
        }
    };
    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let stream = upstream.bytes_stream().map_err(std::io::Error::other);
    let mut response = Response::builder()
        .status(status)
        .body(Body::from_stream(stream))
        .expect("proxy response is valid");
    for name in [
        CONTENT_TYPE,
        WWW_AUTHENTICATE,
        CACHE_CONTROL,
        HeaderName::from_static(PAYMENT_RECEIPT_HEADER),
    ] {
        if let Some(value) = upstream_headers.get(&name) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    response
}

fn merge_nodes(results: Vec<UpstreamResult>) -> BTreeMap<String, NodeRecord> {
    let mut merged = BTreeMap::<String, NodeRecord>::new();
    for node in results
        .into_iter()
        .filter_map(|result| result.nodes.ok())
        .flatten()
    {
        match merged.get(&node.id) {
            Some(existing) if existing.lease_expires_at >= node.lease_expires_at => {}
            _ => {
                merged.insert(node.id.clone(), node);
            }
        }
    }
    merged
}

async fn health(State(state): State<AppState>) -> Response {
    let results = fetch_all(&state, &NodesQuery::default()).await;
    let upstreams: BTreeMap<_, _> = results
        .iter()
        .map(|result| (result.name.clone(), result.nodes.is_ok()))
        .collect();
    let healthy = upstreams.values().filter(|reachable| **reachable).count();
    let (status_code, status) = match healthy {
        0 => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        count if count == upstreams.len() => (StatusCode::OK, "ok"),
        _ => (StatusCode::OK, "degraded"),
    };
    (status_code, Json(HealthResponse { status, upstreams })).into_response()
}

async fn fetch_all(state: &AppState, query: &NodesQuery) -> Vec<UpstreamResult> {
    join_all(state.upstreams.iter().cloned().map(|upstream| {
        let client = state.client.clone();
        let query = query.clone();
        async move {
            let url = format!("{}/nodes", upstream.url.trim_end_matches('/'));
            let nodes = async {
                let response = client.get(url).query(&query).send().await.map_err(|_| ())?;
                if !response.status().is_success() {
                    return Err(());
                }
                response.json::<Vec<NodeRecord>>().await.map_err(|_| ())
            }
            .await;
            UpstreamResult {
                name: upstream.name,
                nodes,
            }
        }
    }))
    .await
}

pub fn parse_upstreams(value: &str) -> Result<Vec<Upstream>, String> {
    let mut upstreams = Vec::new();
    for entry in value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (name, url) = entry
            .split_once('=')
            .ok_or_else(|| "REGISTRY_UPSTREAMS entries must use name=url".to_string())?;
        if name.trim().is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("REGISTRY_UPSTREAMS contains an invalid name or URL".to_string());
        }
        upstreams.push(Upstream {
            name: name.trim().to_string(),
            url: url.trim_end_matches('/').to_string(),
        });
    }
    if upstreams.is_empty() {
        return Err("REGISTRY_UPSTREAMS must contain at least one upstream".to_string());
    }
    Ok(upstreams)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, net::SocketAddr, sync::Arc};

    use axum::{
        body::{Body, Bytes},
        extract::{Query, State},
        http::{header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderMap, Request},
        routing::{get, post},
        Json, Router,
    };
    use mpp::protocol::core::{
        format_authorization, Base64UrlJson, PaymentChallenge, PaymentPayload, Receipt,
    };
    use serde_json::{json, Value};
    use tokio::{net::TcpListener, sync::Mutex};
    use tower::ServiceExt;

    use super::*;

    #[derive(Clone, Default)]
    struct MockCoordinator {
        intents: Arc<Mutex<Vec<(String, String, u64, [u8; 32])>>>,
        redemptions: Arc<Mutex<Vec<(String, String, [u8; 32])>>>,
    }

    fn paused_session() -> SessionRecord {
        SessionRecord {
            session_id: "sess_portable".into(),
            logical_node: "madrid".into(),
            node_url: "https://madrid.test".into(),
            state: tempvpn_coordinator_client::SessionState::Paused,
            phase: None,
            total_seconds: 60,
            remaining_seconds: 60,
            created_at: Utc::now(),
            connected_at: None,
            last_heartbeat_at: None,
            grace_deadline: Utc::now() + chrono::Duration::days(7),
            assigned_ip: None,
            client_public_key: None,
            active_generation_id: None,
        }
    }

    impl FixedSessionCoordinator for MockCoordinator {
        fn create_payment_intent(
            &self,
            intent_id: String,
            node_id: String,
            duration_seconds: u64,
            fingerprint: [u8; 32],
        ) -> BoxFuture<'static, Result<(), CoordinatorError>> {
            let intents = self.intents.clone();
            Box::pin(async move {
                intents
                    .lock()
                    .await
                    .push((intent_id, node_id, duration_seconds, fingerprint));
                Ok(())
            })
        }
        fn redeem_payment(
            &self,
            intent_id: String,
            reference: String,
            fingerprint: [u8; 32],
            _grace: u64,
        ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>> {
            let redemptions = self.redemptions.clone();
            Box::pin(async move {
                redemptions
                    .lock()
                    .await
                    .push((intent_id, reference, fingerprint));
                Ok(paused_session())
            })
        }
        fn status(
            &self,
            _session_id: String,
        ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>> {
            Box::pin(async { Ok(paused_session()) })
        }
        fn heartbeat(
            &self,
            _session_id: String,
        ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>> {
            Box::pin(async { Ok(paused_session()) })
        }
        fn pause(
            &self,
            _session_id: String,
        ) -> BoxFuture<'static, Result<SessionRecord, CoordinatorError>> {
            Box::pin(async { Ok(paused_session()) })
        }
    }

    struct MockChallenger;
    impl ChargeChallenger for MockChallenger {
        fn challenge(
            &self,
            amount: &str,
            _options: ChallengeOptions,
        ) -> Result<PaymentChallenge, String> {
            Ok(PaymentChallenge::new(
                "intent-1", "registry.tempvpn.xyz", "tempo", "charge",
                Base64UrlJson::from_value(&json!({"amount": amount, "currency": "0xcurrency", "recipient": "0xrecipient"})).unwrap(),
            ))
        }
        fn verify_payment(
            &self,
            _credential: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Receipt, String>> + Send>>
        {
            Box::pin(async { Ok(Receipt::success("tempo", "0xpaid")) })
        }
    }

    fn fixed_test_state(upstream: String, coordinator: MockCoordinator) -> AppState {
        AppState::new(
            vec![Upstream {
                name: "test".into(),
                url: upstream,
            }],
            Duration::from_secs(1),
        )
        .unwrap()
        .with_fixed_payments(
            Arc::new(coordinator),
            Arc::new(MockChallenger),
            FixedPaymentSettings {
                realm: "registry.tempvpn.xyz".into(),
                currency: "0xcurrency".into(),
                recipient: "0xrecipient".into(),
                max_duration_seconds: 3600,
                grace_period_seconds: 604800,
                node_control_token: "control-secret".into(),
            },
        )
    }

    #[derive(Clone)]
    struct MockState {
        status: StatusCode,
        body: Value,
        queries: Arc<Mutex<Vec<HashMap<String, String>>>>,
    }

    async fn mock_nodes(
        State(state): State<MockState>,
        Query(query): Query<HashMap<String, String>>,
    ) -> Response {
        state.queries.lock().await.push(query);
        (state.status, Json(state.body)).into_response()
    }

    async fn spawn_mock(
        status: StatusCode,
        body: Value,
    ) -> (String, Arc<Mutex<Vec<HashMap<String, String>>>>) {
        let queries = Arc::new(Mutex::new(Vec::new()));
        let state = MockState {
            status,
            body,
            queries: queries.clone(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/nodes", get(mock_nodes))
                    .with_state(state),
            )
            .await
            .unwrap();
        });
        (format!("http://{addr}"), queries)
    }

    #[derive(Clone)]
    struct GatewayMockState {
        api_url: String,
        requests: Arc<Mutex<Vec<(Option<String>, Value)>>>,
    }

    async fn gateway_nodes(State(state): State<GatewayMockState>) -> Json<Value> {
        Json(json!([{
            "id": "madrid",
            "name": "Madrid",
            "region": "test",
            "api_url": state.api_url,
            "wireguard_endpoint": "madrid.test:51820",
            "expected_exit_ip": "127.0.0.1",
            "accepting_sessions": true,
            "available_slots": 10,
            "lease_expires_at": "2030-01-01T00:00:00Z"
        }]))
    }

    async fn gateway_session(
        State(state): State<GatewayMockState>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let authorization = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = serde_json::from_slice(&body).unwrap();
        state.requests.lock().await.push((authorization, body));
        (
            StatusCode::PAYMENT_REQUIRED,
            [(WWW_AUTHENTICATE, "Payment realm=\"tempvpn.xyz\"")],
            Json(json!({ "error": "payment required" })),
        )
            .into_response()
    }

    async fn gateway_connect(
        State(state): State<GatewayMockState>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let mut body: Value = serde_json::from_slice(&body).unwrap();
        body["control_token"] = headers
            .get(CONTROL_PLANE_TOKEN_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(Value::from)
            .unwrap_or(Value::Null);
        state.requests.lock().await.push((None, body));
        Json(json!({ "session_id": "sess_portable", "state": "active" })).into_response()
    }

    async fn gateway_stream_head(
        State(state): State<GatewayMockState>,
        OriginalUri(uri): OriginalUri,
        headers: HeaderMap,
    ) -> Response {
        let authorization = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        state
            .requests
            .lock()
            .await
            .push((authorization, json!({ "path_and_query": uri.to_string() })));
        (
            StatusCode::NO_CONTENT,
            [(HeaderName::from_static(PAYMENT_RECEIPT_HEADER), "receipt")],
        )
            .into_response()
    }

    async fn spawn_gateway_mock() -> (String, Arc<Mutex<Vec<(Option<String>, Value)>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = GatewayMockState {
            api_url: format!("http://{addr}"),
            requests: requests.clone(),
        };
        tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/nodes", get(gateway_nodes))
                    .route("/sessions", post(gateway_session))
                    .route("/sessions/{session_id}/connect", post(gateway_connect))
                    .route(
                        "/sessions/stream",
                        post(gateway_session).head(gateway_stream_head),
                    )
                    .with_state(state),
            )
            .await
            .unwrap();
        });
        (format!("http://{addr}"), requests)
    }

    fn node(id: &str, lease: &str, name: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "region": "test",
            "api_url": format!("http://{id}:8080"),
            "wireguard_endpoint": format!("{id}:51820"),
            "expected_exit_ip": "127.0.0.1",
            "accepting_sessions": true,
            "available_slots": 10,
            "lease_expires_at": lease
        })
    }

    async fn response_json(response: Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn paid_gateway_resolves_node_and_preserves_payment_headers() {
        let (upstream, requests) = spawn_gateway_mock().await;
        let app = router(
            AppState::new(
                vec![Upstream {
                    name: "test".into(),
                    url: upstream,
                }],
                Duration::from_secs(1),
            )
            .unwrap(),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sessions")
                    .header(CONTENT_TYPE, "application/json")
                    .header(AUTHORIZATION, "Payment credential")
                    .body(Body::from(r#"{"node_id":"madrid","duration_seconds":300}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
        assert_eq!(
            response.headers()[WWW_AUTHENTICATE],
            "Payment realm=\"tempvpn.xyz\""
        );
        let seen = requests.lock().await;
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0.as_deref(), Some("Payment credential"));
        assert_eq!(seen[0].1["node_id"], "madrid");
        assert_eq!(seen[0].1["duration_seconds"], 300);
    }

    #[tokio::test]
    async fn paid_gateway_requires_a_selected_node() {
        let response = router(
            AppState::new(
                vec![Upstream {
                    name: "test".into(),
                    url: "http://127.0.0.1:1".into(),
                }],
                Duration::from_secs(1),
            )
            .unwrap(),
        )
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/sessions")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"duration_seconds":300}"#))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(response).await["error"],
            "node_id is required"
        );
    }

    #[tokio::test]
    async fn registry_owns_fixed_challenge_price_redemption_and_receipt() {
        let (upstream, _) = spawn_gateway_mock().await;
        let coordinator = MockCoordinator::default();
        let app = router(fixed_test_state(upstream, coordinator.clone()));
        let unpaid = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sessions")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"node_id":"madrid","duration_seconds":120}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unpaid.status(), StatusCode::PAYMENT_REQUIRED);
        let challenge =
            PaymentChallenge::from_header(unpaid.headers()[WWW_AUTHENTICATE].to_str().unwrap())
                .unwrap();
        assert_eq!(challenge.realm, "registry.tempvpn.xyz");
        assert_eq!(challenge.request.decode_value().unwrap()["amount"], "0.02");
        assert_eq!(coordinator.intents.lock().await.len(), 1);

        let credential =
            PaymentCredential::new(challenge.to_echo(), PaymentPayload::transaction("0xabc"));
        let authorization = format_authorization(&credential).unwrap();
        let paid = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sessions")
                    .header(CONTENT_TYPE, "application/json")
                    .header(AUTHORIZATION, authorization)
                    .body(Body::from(r#"{"node_id":"madrid","duration_seconds":120}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(paid.status(), StatusCode::CREATED);
        let receipt =
            Receipt::from_header(paid.headers()[PAYMENT_RECEIPT_HEADER].to_str().unwrap()).unwrap();
        assert_eq!(receipt.reference, "0xpaid");
        let paid_document = response_json(paid).await;
        assert_eq!(paid_document["session_id"], "sess_portable");
        assert_eq!(paid_document["node_id"], "madrid");
        assert!(paid_document.get("not_after").is_some());
        assert!(paid_document.get("grace_deadline").is_none());
        assert_eq!(coordinator.redemptions.lock().await.len(), 1);

        // If the first 201 is lost, replaying the same paid request asks the
        // durable coordinator for the already-created portable session.
        let replay = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sessions")
                    .header(CONTENT_TYPE, "application/json")
                    .header(AUTHORIZATION, format_authorization(&credential).unwrap())
                    .body(Body::from(r#"{"node_id":"madrid","duration_seconds":120}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::CREATED);
        assert!(replay.headers().contains_key(PAYMENT_RECEIPT_HEADER));
        assert_eq!(response_json(replay).await["session_id"], "sess_portable");
        assert_eq!(coordinator.redemptions.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn registry_rejects_partial_minutes_before_challenge() {
        let (upstream, _) = spawn_gateway_mock().await;
        let coordinator = MockCoordinator::default();
        let response = router(fixed_test_state(upstream, coordinator.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sessions")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"node_id":"madrid","duration_seconds":61}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response.headers().get(WWW_AUTHENTICATE).is_none());
        assert!(coordinator.intents.lock().await.is_empty());
    }

    #[tokio::test]
    async fn registry_rejects_zero_capacity_before_challenge() {
        let (upstream, _) = spawn_mock(
            StatusCode::OK,
            json!([{
                "id": "full-node",
                "api_url": "https://full-node.test",
                "accepting_sessions": true,
                "available_slots": 0,
                "lease_expires_at": "2030-01-01T00:00:00Z"
            }]),
        )
        .await;
        let coordinator = MockCoordinator::default();
        let response = router(fixed_test_state(upstream, coordinator.clone()))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sessions")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"node_id":"full-node","duration_seconds":60}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(response.headers().get(WWW_AUTHENTICATE).is_none());
        assert!(coordinator.intents.lock().await.is_empty());
    }

    #[tokio::test]
    async fn fixed_lifecycle_reads_coordinator_at_registry() {
        let (upstream, _) = spawn_gateway_mock().await;
        let coordinator = MockCoordinator::default();
        let app = router(fixed_test_state(upstream, coordinator));
        for (method, path) in [
            (Method::GET, "/sessions/sess_portable/status"),
            (Method::POST, "/sessions/sess_portable/heartbeat"),
            (Method::POST, "/sessions/sess_portable/pause"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response_json(response).await["session_id"], "sess_portable");
        }
    }

    #[tokio::test]
    async fn registry_authenticates_fixed_activation_to_selected_node() {
        let (upstream, requests) = spawn_gateway_mock().await;
        let coordinator = MockCoordinator::default();
        let response = router(fixed_test_state(upstream, coordinator))
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sessions/sess_portable/connect")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"node_id":"madrid","client_public_key":"client-key"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let seen = requests.lock().await;
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].1["control_token"], "control-secret");
        assert_eq!(seen[0].1["client_public_key"], "client-key");
    }

    #[tokio::test]
    async fn streaming_head_keeps_node_affinity_query_and_payment_headers() {
        let (upstream, requests) = spawn_gateway_mock().await;
        let response = router(
            AppState::new(
                vec![Upstream {
                    name: "test".into(),
                    url: upstream,
                }],
                Duration::from_secs(1),
            )
            .unwrap(),
        )
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/sessions/stream?node_id=madrid&client_public_key=key&duration_seconds=60")
                .header(AUTHORIZATION, "Payment voucher")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(response.headers()[PAYMENT_RECEIPT_HEADER], "receipt");
        let seen = requests.lock().await;
        assert_eq!(seen[0].0.as_deref(), Some("Payment voucher"));
        assert_eq!(
            seen[0].1["path_and_query"],
            "/sessions/stream?node_id=madrid&client_public_key=key&duration_seconds=60"
        );
    }

    #[tokio::test]
    async fn openapi_documents_the_complete_fixed_session_lifecycle() {
        let response = router(
            AppState::new(
                vec![Upstream {
                    name: "one".into(),
                    url: "http://127.0.0.1:1".into(),
                }],
                Duration::from_secs(1),
            )
            .unwrap(),
        )
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .header("origin", "https://tempvpn.xyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[CONTENT_TYPE],
            "application/json; charset=utf-8"
        );
        assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN], "*");

        let document = response_json(response).await;
        assert_eq!(document["openapi"], "3.1.0");
        assert!(document["paths"]["/nodes"]["get"].is_object());
        assert!(document["paths"]["/sessions"]["post"].is_object());
        let create = &document["paths"]["/sessions"]["post"];
        assert!(create["description"]
            .as_str()
            .unwrap()
            .contains("runtime MPP 402 Challenge is authoritative"));
        let duration = &document["components"]["schemas"]["CreateSessionRequest"]["properties"]
            ["duration_seconds"];
        assert_eq!(duration["minimum"], 60);
        assert_eq!(duration["multipleOf"], 60);
        assert!(duration["description"]
            .as_str()
            .unwrap()
            .contains("$0.01 per minute"));

        let connect = &document["paths"]["/sessions/{session_id}/connect"]["post"];
        assert_eq!(connect["operationId"], "connectFixedSession");
        assert_eq!(
            connect["parameters"][0]["$ref"],
            "#/components/parameters/SessionId"
        );
        assert_eq!(connect["requestBody"]["required"], true);
        assert_eq!(
            connect["requestBody"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/ConnectSessionRequest"
        );
        assert_eq!(
            connect["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/Session"
        );
        assert!(connect["responses"]["409"].is_object());

        let pause = &document["paths"]["/sessions/{session_id}/pause"]["post"];
        assert_eq!(pause["operationId"], "pauseFixedSession");
        assert_eq!(
            pause["parameters"][0]["$ref"],
            "#/components/parameters/SessionId"
        );
        assert!(pause.get("requestBody").is_none());
        assert!(pause["responses"]["200"].is_object());
        assert!(pause["responses"]["404"].is_object());
        assert_eq!(
            document["paths"]["/sessions"]["post"]["responses"]["201"]["content"]
                ["application/json"]["schema"]["$ref"],
            "#/components/schemas/PortableSession"
        );

        for operation in [&document["paths"]["/sessions"]["post"], connect, pause] {
            assert!(operation.get("servers").is_none());
        }

        let create_request = &document["components"]["schemas"]["CreateSessionRequest"];
        assert!(create_request["required"]
            .as_array()
            .unwrap()
            .contains(&json!("node_id")));
        let connect_request = &document["components"]["schemas"]["ConnectSessionRequest"];
        assert!(connect_request["required"]
            .as_array()
            .unwrap()
            .contains(&json!("node_id")));
        assert!(connect_request["required"]
            .as_array()
            .unwrap()
            .contains(&json!("client_public_key")));
        let stream_request = &document["components"]["schemas"]["StreamSessionRequest"];
        assert!(stream_request["required"]
            .as_array()
            .unwrap()
            .contains(&json!("node_id")));
        let stream_head = &document["paths"]["/sessions/stream"]["head"];
        assert!(document["paths"]["/sessions/stream"].get("get").is_none());
        assert_eq!(stream_head["operationId"], "manageStreamingSession");
        assert!(stream_head["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|parameter| parameter["name"] == "node_id"));
        assert_eq!(
            document["components"]["parameters"]["SessionId"]["required"],
            true
        );

        let session = &document["components"]["schemas"]["Session"];
        let portable = &document["components"]["schemas"]["PortableSession"];
        assert!(portable["required"]
            .as_array()
            .unwrap()
            .contains(&json!("node_id")));
        assert!(portable["properties"].get("server_public_key").is_none());
        assert!(session["properties"]["client_public_key"]["type"]
            .as_array()
            .unwrap()
            .contains(&json!("null")));
        assert!(session["properties"]["assigned_ip"]["type"]
            .as_array()
            .unwrap()
            .contains(&json!("null")));
        assert_eq!(
            document["components"]["examples"]["PausedSession"]["value"]["state"],
            "paused"
        );
        assert!(
            document["components"]["examples"]["PausedSession"]["value"]["client_public_key"]
                .is_null()
        );
        assert!(
            document["components"]["examples"]["PausedSession"]["value"]["assigned_ip"].is_null()
        );
    }

    #[tokio::test]
    async fn redirects_human_docs_and_serves_agent_markdown() {
        let app = router(AppState::new(Vec::new(), Duration::from_secs(1)).unwrap());
        let redirect = app
            .clone()
            .oneshot(Request::builder().uri("/docs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(redirect.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(
            redirect.headers()[axum::http::header::LOCATION],
            "https://tempvpn.xyz/docs/"
        );

        let markdown = app
            .oneshot(
                Request::builder()
                    .uri("/docs/markdown")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(markdown.status(), StatusCode::OK);
        assert_eq!(
            markdown.headers()[CONTENT_TYPE],
            "text/plain; charset=utf-8"
        );
        let body = axum::body::to_bytes(markdown.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.contains("registry is the control-plane origin"));
        assert!(body.contains("POST /sessions/{session_id}/heartbeat"));

        let compatibility_alias =
            router(AppState::new(Vec::new(), Duration::from_secs(1)).unwrap())
                .oneshot(
                    Request::builder()
                        .uri("/docs/markdown.md")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
        assert_eq!(compatibility_alias.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn merges_six_nodes_forwards_filters_and_resolves_duplicates() {
        let (first, first_queries) = spawn_mock(
            StatusCode::OK,
            json!([
                node("a", "2030-01-01T00:00:00Z", "old"),
                node("b", "2030-01-01T00:00:00Z", "b"),
                node("c", "2030-01-01T00:00:00Z", "c")
            ]),
        )
        .await;
        let (second, _) = spawn_mock(
            StatusCode::OK,
            json!([
                node("a", "2031-01-01T00:00:00Z", "new"),
                node("d", "2030-01-01T00:00:00Z", "d"),
                node("e", "2030-01-01T00:00:00Z", "e"),
                node("f", "2030-01-01T00:00:00Z", "f")
            ]),
        )
        .await;
        let app = router(
            AppState::new(
                vec![
                    Upstream {
                        name: "one".into(),
                        url: first,
                    },
                    Upstream {
                        name: "two".into(),
                        url: second,
                    },
                ],
                Duration::from_secs(1),
            )
            .unwrap(),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/nodes?country=US&available=true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[DEGRADED_HEADER], "false");
        let body = response_json(response).await;
        let nodes = body.as_array().unwrap();
        assert_eq!(nodes.len(), 6);
        assert_eq!(nodes[0]["id"], "a");
        assert_eq!(nodes[0]["name"], "new");
        let query = &first_queries.lock().await[0];
        assert_eq!(query.get("country").map(String::as_str), Some("US"));
        assert_eq!(query.get("available").map(String::as_str), Some("true"));
    }

    #[tokio::test]
    async fn serves_partial_catalog_and_reports_degraded_health() {
        let (healthy, _) = spawn_mock(
            StatusCode::OK,
            json!([node("a", "2030-01-01T00:00:00Z", "a")]),
        )
        .await;
        let (failed, _) = spawn_mock(StatusCode::BAD_GATEWAY, json!({"error":"no"})).await;
        let app = router(
            AppState::new(
                vec![
                    Upstream {
                        name: "healthy".into(),
                        url: healthy,
                    },
                    Upstream {
                        name: "failed".into(),
                        url: failed,
                    },
                ],
                Duration::from_secs(1),
            )
            .unwrap(),
        );
        let nodes_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/nodes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(nodes_response.status(), StatusCode::OK);
        assert_eq!(nodes_response.headers()[DEGRADED_HEADER], "true");
        let health_response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health_response.status(), StatusCode::OK);
        let health = response_json(health_response).await;
        assert_eq!(health["status"], "degraded");
        assert_eq!(health["upstreams"]["healthy"], true);
        assert_eq!(health["upstreams"]["failed"], false);
    }

    #[tokio::test]
    async fn returns_503_when_all_upstreams_fail() {
        let (first, _) = spawn_mock(StatusCode::INTERNAL_SERVER_ERROR, json!([])).await;
        let (second, _) = spawn_mock(StatusCode::SERVICE_UNAVAILABLE, json!([])).await;
        let app = router(
            AppState::new(
                vec![
                    Upstream {
                        name: "one".into(),
                        url: first,
                    },
                    Upstream {
                        name: "two".into(),
                        url: second,
                    },
                ],
                Duration::from_secs(1),
            )
            .unwrap(),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/nodes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response_json(response).await["error"],
            "all registry upstreams are unavailable"
        );
    }

    #[tokio::test]
    async fn adds_public_cors_headers() {
        let (upstream, _) = spawn_mock(StatusCode::OK, json!([])).await;
        let app = router(
            AppState::new(
                vec![Upstream {
                    name: "one".into(),
                    url: upstream,
                }],
                Duration::from_secs(1),
            )
            .unwrap(),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/nodes")
                    .header("origin", "https://tempvpn.xyz")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    }

    #[test]
    fn parses_named_upstreams() {
        let upstreams = parse_upstreams("americas=http://one,europe-asia=https://two/").unwrap();
        assert_eq!(upstreams.len(), 2);
        assert_eq!(upstreams[1].name, "europe-asia");
        assert_eq!(upstreams[1].url, "https://two");
        assert!(parse_upstreams("bad").is_err());
    }
}
