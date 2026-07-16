use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use mpp::server::axum::{ChargeChallenger, ChargeConfig, MppCharge};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    config::Config,
    helpers::{bearer_token, registry_token_matches},
    registry::{NodeAdvertisement, Registry},
    sessions::Sessions,
};

const VPN_SESSION_PRICE_AMOUNT: &str = "0.01";

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub sessions: Arc<Sessions>,
    pub challenger: Arc<dyn ChargeChallenger>,
    pub registry: Registry,
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

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    active_sessions: usize,
}

pub fn router(state: AppState) -> Router {
    Router::new()
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
        )
        .with_state(state)
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
        return response.into_response();
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
        return response.into_response();
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

async fn get_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    if let Err(response) = authorize(&state.config, &headers) {
        return response.into_response();
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
        return response.into_response();
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

fn authorize(
    config: &Config,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
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
        ))
    }
}

fn authorize_registry(
    config: &Config,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let bearer = bearer_token(headers);
    if registry_token_matches(config.registry_token.as_deref(), bearer) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing or invalid registry token" })),
        ))
    }
}
