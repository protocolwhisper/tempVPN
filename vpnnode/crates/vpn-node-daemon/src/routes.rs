use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{config::Config, sessions::Sessions};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub sessions: Arc<Sessions>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub client_public_key: String,
    pub duration_seconds: u64,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    active_sessions: usize,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sessions", post(create_session))
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

async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateSessionRequest>,
) -> Response {
    if let Err(response) = authorize(&state.config, &headers) {
        return response;
    }

    match state
        .sessions
        .create(request.client_public_key, request.duration_seconds)
        .await
    {
        Ok(session) => (StatusCode::CREATED, Json(session)).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
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

fn authorize(config: &Config, headers: &HeaderMap) -> Result<(), Response> {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
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
