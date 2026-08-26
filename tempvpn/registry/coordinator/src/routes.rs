use axum::{
    body::Body,
    extract::{Extension, Query, State},
    http::{Method, Request as HttpRequest, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};

use crate::{
    types::{
        ApiAcknowledgement, AppState, CertificateRenewalRequest, CoordinationIdentity,
        EnrollmentRequest, EnrollmentTokenRequest, GenerationIdentityRequest,
        GenerationRegistration, GenerationRenewalRequest, NodeRecord, PaymentIntentRequest,
        PaymentRedemptionRequest, PeerAcknowledgementRequest, SessionClaimRequest,
        SessionTokenRequest, COORDINATION_API_V1,
    },
    Error, Result,
};

#[derive(Debug, Default, Deserialize)]
struct NodesQuery {
    country: Option<String>,
    city: Option<String>,
    region: Option<String>,
    available: Option<bool>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/nodes", get(nodes))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::OPTIONS]),
        )
        .with_state(state)
}

pub fn coordination_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/generations/register", post(register_generation))
        .route("/generations/renew", post(renew_generation))
        .route("/generations/promote", post(promote_generation))
        .route("/generations/drain", post(drain_generation))
        .route("/generations/drain-status", post(drain_status))
        .route("/payment-intents", post(create_payment_intent))
        .route("/payments/redeem", post(redeem_payment))
        .route("/sessions/status", post(session_status))
        .route("/sessions/heartbeat", post(heartbeat_session))
        .route("/sessions/claim", post(claim_session))
        .route("/sessions/activation-failed", post(fail_activation))
        .route("/sessions/pause", post(pause_session))
        .route("/sessions/terminate", post(terminate_session))
        .route("/peers/snapshot", post(peer_snapshot))
        .route("/peers/acknowledge", post(acknowledge_peers))
        .route("/enrollment-tokens", post(create_enrollment_token))
        .route("/certificates/renew", post(renew_certificate))
        .route_layer(middleware::from_fn(authenticate_certificate));
    Router::new()
        .nest(
            COORDINATION_API_V1,
            Router::new()
                .route("/enroll", post(enroll))
                .merge(protected),
        )
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Response {
    match state.store.health().await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "database": "ok" })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "unavailable", "database": "unavailable" })),
        )
            .into_response(),
    }
}

async fn nodes(State(state): State<AppState>, Query(query): Query<NodesQuery>) -> Response {
    match state.store.nodes().await {
        Ok(nodes) => Json(filter_nodes(nodes, &query)).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "coordinator database is unavailable" })),
        )
            .into_response(),
    }
}

fn filter_nodes(nodes: Vec<NodeRecord>, query: &NodesQuery) -> Vec<NodeRecord> {
    nodes
        .into_iter()
        .filter(|node| {
            query
                .country
                .as_deref()
                .is_none_or(|value| node.country_code.as_deref() == Some(value))
                && query
                    .city
                    .as_deref()
                    .is_none_or(|value| node.city.as_deref() == Some(value))
                && query
                    .region
                    .as_deref()
                    .is_none_or(|value| node.region == value)
                && query
                    .available
                    .is_none_or(|value| !value || node.available_slots > 0)
        })
        .collect()
}

async fn authenticate_certificate(mut request: HttpRequest<Body>, next: Next) -> Response {
    if request.extensions().get::<CoordinationIdentity>().is_none() {
        let Some(certificates) = request
            .extensions()
            .get::<axum_server_mtls::PeerCertificates>()
        else {
            return Error::Unauthorized.into_response();
        };
        let Some(certificate) = certificates.leaf() else {
            return Error::Unauthorized.into_response();
        };
        let identity = match crate::pki::identity_from_certificate(certificate) {
            Ok(identity) => identity,
            Err(error) => return error.into_response(),
        };
        request.extensions_mut().insert(identity);
    }
    next.run(request).await
}

async fn register_generation(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<GenerationRegistration>,
) -> Result<Json<ApiAcknowledgement>> {
    identity.require_node(&request.logical_node, &request.generation_id)?;
    state.store.register_generation(&request).await?;
    Ok(Json(ApiAcknowledgement::ok()))
}

async fn renew_generation(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<GenerationRenewalRequest>,
) -> Result<Json<ApiAcknowledgement>> {
    identity.require_node(&request.logical_node, &request.generation_id)?;
    state
        .store
        .renew_generation(
            &request.logical_node,
            &request.generation_id,
            request.available_slots,
            request.health_expires_at,
        )
        .await?;
    Ok(Json(ApiAcknowledgement::ok()))
}

async fn promote_generation(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<GenerationIdentityRequest>,
) -> Result<Json<ApiAcknowledgement>> {
    identity.require_operator()?;
    state
        .store
        .promote_generation(&request.logical_node, &request.generation_id)
        .await?;
    Ok(Json(ApiAcknowledgement::ok()))
}

async fn drain_generation(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<GenerationIdentityRequest>,
) -> Result<Json<ApiAcknowledgement>> {
    identity.require_operator()?;
    state
        .store
        .drain_generation(&request.logical_node, &request.generation_id)
        .await?;
    Ok(Json(ApiAcknowledgement::ok()))
}

async fn drain_status(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<GenerationIdentityRequest>,
) -> Result<Json<crate::types::DrainStatus>> {
    identity.require_operator()?;
    Ok(Json(
        state
            .store
            .drain_status(&request.logical_node, &request.generation_id)
            .await?,
    ))
}

async fn create_payment_intent(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<PaymentIntentRequest>,
) -> Result<Json<crate::types::PaymentIntent>> {
    identity.require_control_plane_or_logical_node(&request.logical_node)?;
    Ok(Json(
        state
            .store
            .create_payment_intent(
                request.intent_id.as_deref(),
                &request.logical_node,
                request.duration_seconds,
                request.request_fingerprint,
                request.challenge_key_version,
                request.expires_at,
            )
            .await?,
    ))
}

async fn redeem_payment(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<PaymentRedemptionRequest>,
) -> Result<Json<crate::types::SessionRecord>> {
    let logical_node = state
        .store
        .payment_intent_logical_node(&request.intent_id)
        .await?;
    identity.require_control_plane_or_logical_node(&logical_node)?;
    Ok(Json(
        state
            .store
            .redeem_payment(
                &request.intent_id,
                &request.payment_method,
                &request.transaction_reference,
                request.request_fingerprint,
                request.grace_seconds,
                &state.token_cipher,
            )
            .await?,
    ))
}

async fn session_status(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<SessionTokenRequest>,
) -> Result<Json<crate::types::SessionRecord>> {
    identity.require_control_plane_or_node()?;
    Ok(Json(
        state
            .store
            .session_status(&request.session_id, &state.token_cipher)
            .await?,
    ))
}

async fn heartbeat_session(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<SessionTokenRequest>,
) -> Result<Json<crate::types::SessionRecord>> {
    identity.require_control_plane_or_node()?;
    Ok(Json(
        state
            .store
            .heartbeat_session(&request.session_id, &state.token_cipher)
            .await?,
    ))
}

async fn claim_session(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<SessionClaimRequest>,
) -> Result<Json<crate::types::ActivationClaim>> {
    let (logical_node, generation_id) = identity.node_scope()?;
    if generation_id != request.generation_id {
        return Err(Error::Forbidden);
    }
    Ok(Json(
        state
            .store
            .claim_session(
                &request.session_id,
                &request.client_public_key,
                logical_node,
                &request.generation_id,
                &state.token_cipher,
            )
            .await?,
    ))
}

async fn fail_activation(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<SessionTokenRequest>,
) -> Result<Json<ApiAcknowledgement>> {
    authorize_session(&state, &identity, &request.session_id).await?;
    state.store.fail_activation(&request.session_id).await?;
    Ok(Json(ApiAcknowledgement::ok()))
}

async fn pause_session(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<SessionTokenRequest>,
) -> Result<Json<crate::types::SessionRecord>> {
    identity.require_control_plane_or_node()?;
    Ok(Json(
        state
            .store
            .pause_session(&request.session_id, &state.token_cipher)
            .await?,
    ))
}

async fn terminate_session(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<SessionTokenRequest>,
) -> Result<Json<crate::types::SessionRecord>> {
    identity.require_operator()?;
    Ok(Json(
        state
            .store
            .terminate_session(&request.session_id, &state.token_cipher)
            .await?,
    ))
}

async fn peer_snapshot(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<GenerationIdentityRequest>,
) -> Result<Json<crate::types::PeerSnapshot>> {
    identity.require_node(&request.logical_node, &request.generation_id)?;
    Ok(Json(
        state
            .store
            .peer_snapshot(
                &request.logical_node,
                &request.generation_id,
                &state.token_cipher,
            )
            .await?,
    ))
}

async fn acknowledge_peers(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<PeerAcknowledgementRequest>,
) -> Result<Json<ApiAcknowledgement>> {
    identity.require_node(&request.logical_node, &request.generation_id)?;
    state
        .store
        .acknowledge_peers(
            &request.logical_node,
            &request.generation_id,
            request.applied_revision,
            request.actual_peer_count,
        )
        .await?;
    Ok(Json(ApiAcknowledgement::ok()))
}

async fn create_enrollment_token(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<EnrollmentTokenRequest>,
) -> Result<Json<crate::types::EnrollmentTokenResponse>> {
    identity.require_operator()?;
    Ok(Json(
        state
            .store
            .create_enrollment_token(&request.logical_node, &request.generation_id)
            .await?,
    ))
}

async fn enroll(
    State(state): State<AppState>,
    Json(request): Json<EnrollmentRequest>,
) -> Result<Json<crate::types::CertificateResponse>> {
    let authority = state
        .certificate_authority
        .as_ref()
        .ok_or_else(|| Error::Config("certificate authority is unavailable".into()))?;
    let identity = CoordinationIdentity::Node {
        logical_node: request.logical_node.clone(),
        generation_id: request.generation_id.clone(),
    };
    let certificate = authority.issue(&request.certificate_signing_request_pem, &identity)?;
    state
        .store
        .consume_enrollment_token(
            &request.enrollment_token,
            &request.logical_node,
            &request.generation_id,
        )
        .await?;
    Ok(Json(certificate))
}

async fn renew_certificate(
    State(state): State<AppState>,
    Extension(identity): Extension<CoordinationIdentity>,
    Json(request): Json<CertificateRenewalRequest>,
) -> Result<Json<crate::types::CertificateResponse>> {
    let authority = state
        .certificate_authority
        .as_ref()
        .ok_or_else(|| Error::Config("certificate authority is unavailable".into()))?;
    Ok(Json(authority.issue(
        &request.certificate_signing_request_pem,
        &identity,
    )?))
}

async fn authorize_session(
    state: &AppState,
    identity: &CoordinationIdentity,
    session_id: &str,
) -> Result<()> {
    let logical_node = state.store.session_logical_node(session_id).await?;
    identity.require_logical_node(&logical_node)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use chrono::{Duration, Utc};
    use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
    use serde::de::DeserializeOwned;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use crate::{
        crypto::TokenCipher,
        store::Store,
        types::{ActivationClaim, PaymentIntent, PeerSnapshot, SessionRecord},
    };

    use super::*;

    #[tokio::test]
    async fn public_health_reports_database_readiness() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn versioned_private_api_drives_generation_payment_session_and_peer_lifecycle() {
        let app = test_coordination_app();
        let health_expires_at = Utc::now() + Duration::minutes(5);
        let registration = json!({
            "logical_node": "node-a",
            "generation_id": "green",
            "node_name": "Node A",
            "region": "test",
            "country_code": "US",
            "subdivision_code": null,
            "city": "Test City",
            "api_url": "https://node-a.test",
            "wireguard_endpoint": "green.test:51820",
            "wireguard_public_key": "green-server-key",
            "expected_exit_ip": "192.0.2.1",
            "tunnel_network": "10.90.0.0/24",
            "available_slots": 253,
            "health_expires_at": health_expires_at,
        });
        let acknowledgement: ApiAcknowledgement =
            post_json(&app, "/coordination/v1/generations/register", registration).await;
        assert!(acknowledgement.ok);
        let _: ApiAcknowledgement = post_json(
            &app,
            "/coordination/v1/generations/promote",
            json!({"logical_node": "node-a", "generation_id": "green"}),
        )
        .await;

        let fingerprint = [7_u8; 32];
        let intent: PaymentIntent = post_json(
            &app,
            "/coordination/v1/payment-intents",
            json!({
                "logical_node": "node-a",
                "intent_id": null,
                "duration_seconds": 120,
                "request_fingerprint": fingerprint,
                "challenge_key_version": 1,
                "expires_at": Utc::now() + Duration::minutes(5),
            }),
        )
        .await;
        let session: SessionRecord = post_json(
            &app,
            "/coordination/v1/payments/redeem",
            json!({
                "intent_id": intent.intent_id,
                "payment_method": "tempo",
                "transaction_reference": "tx-api-test",
                "request_fingerprint": fingerprint,
                "grace_seconds": 600,
            }),
        )
        .await;
        assert!(session.session_id.starts_with("sess_"));

        let claim: ActivationClaim = post_json(
            &app,
            "/coordination/v1/sessions/claim",
            json!({
                "session_id": session.session_id,
                "client_public_key": "client-key",
                "generation_id": "green",
            }),
        )
        .await;
        assert_eq!(claim.generation_id, "green");
        let snapshot: PeerSnapshot = post_json(
            &app,
            "/coordination/v1/peers/snapshot",
            json!({"logical_node": "node-a", "generation_id": "green"}),
        )
        .await;
        assert_eq!(snapshot.peers.len(), 1);
        let _: ApiAcknowledgement = post_json(
            &app,
            "/coordination/v1/peers/acknowledge",
            json!({
                "logical_node": "node-a",
                "generation_id": "green",
                "applied_revision": snapshot.revision,
                "actual_peer_count": 1,
            }),
        )
        .await;
        let active: SessionRecord = post_json(
            &app,
            "/coordination/v1/sessions/status",
            json!({"session_id": session.session_id}),
        )
        .await;
        assert_eq!(active.state, crate::types::SessionState::Active);
    }

    #[tokio::test]
    async fn private_api_maps_store_conflicts_without_exposing_internal_errors() {
        let app = test_coordination_app();
        let mut request = Request::builder()
            .method("POST")
            .uri("/coordination/v1/sessions/heartbeat")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({"session_id": "sess_unknown"})).unwrap(),
            ))
            .unwrap();
        request.extensions_mut().insert(node_identity());
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body: Value = response_json(response).await;
        assert_eq!(body["error"], "session not found");
    }

    #[tokio::test]
    async fn enrollment_is_single_use_and_authorization_is_generation_scoped() {
        let authority = test_authority();
        let app = coordination_router(AppState {
            store: Store::open_in_memory().unwrap(),
            token_cipher: Arc::new(TokenCipher::new(&[3_u8; 32], 1).unwrap()),
            certificate_authority: Some(authority.clone()),
        });
        let token: crate::types::EnrollmentTokenResponse = post_json(
            &app,
            "/coordination/v1/enrollment-tokens",
            json!({"logical_node": "node-a", "generation_id": "green"}),
        )
        .await;
        let subject_key = KeyPair::generate().unwrap();
        let csr = CertificateParams::default()
            .serialize_request(&subject_key)
            .unwrap()
            .pem()
            .unwrap();
        let enrollment = json!({
            "enrollment_token": token.enrollment_token,
            "logical_node": "node-a",
            "generation_id": "green",
            "certificate_signing_request_pem": csr,
        });
        let response =
            request_json(&app, "/coordination/v1/enroll", enrollment.clone(), None).await;
        assert_eq!(response.status(), StatusCode::OK);
        let issued: crate::types::CertificateResponse = response_json(response).await;
        let certificates = rustls_pemfile::certs(&mut std::io::BufReader::new(
            issued.certificate_chain_pem.as_bytes(),
        ))
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
        assert_eq!(
            crate::pki::identity_from_certificate(&certificates[0]).unwrap(),
            node_identity()
        );

        let renewal_key = KeyPair::generate().unwrap();
        let renewal_csr = CertificateParams::default()
            .serialize_request(&renewal_key)
            .unwrap()
            .pem()
            .unwrap();
        let renewed = request_json(
            &app,
            "/coordination/v1/certificates/renew",
            json!({"certificate_signing_request_pem": renewal_csr}),
            Some(node_identity()),
        )
        .await;
        assert_eq!(renewed.status(), StatusCode::OK);
        let renewed: crate::types::CertificateResponse = response_json(renewed).await;
        let renewed_certificates = rustls_pemfile::certs(&mut std::io::BufReader::new(
            renewed.certificate_chain_pem.as_bytes(),
        ))
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
        assert_eq!(
            crate::pki::identity_from_certificate(&renewed_certificates[0]).unwrap(),
            node_identity()
        );

        let replay = request_json(&app, "/coordination/v1/enroll", enrollment, None).await;
        assert_eq!(replay.status(), StatusCode::FORBIDDEN);

        let unauthorized = request_json(
            &app,
            "/coordination/v1/generations/drain-status",
            json!({"logical_node": "node-a", "generation_id": "green"}),
            Some(node_identity()),
        )
        .await;
        assert_eq!(unauthorized.status(), StatusCode::FORBIDDEN);

        let cross_scope = request_json(
            &app,
            "/coordination/v1/generations/register",
            json!({
                "logical_node": "node-b",
                "generation_id": "green",
                "node_name": "Node B",
                "region": "test",
                "country_code": null,
                "subdivision_code": null,
                "city": null,
                "api_url": "https://node-b.test",
                "wireguard_endpoint": "node-b.test:51820",
                "wireguard_public_key": "server-key",
                "expected_exit_ip": "192.0.2.2",
                "tunnel_network": "10.91.0.0/24",
                "available_slots": 253,
                "health_expires_at": Utc::now() + Duration::minutes(5),
            }),
            Some(node_identity()),
        )
        .await;
        assert_eq!(cross_scope.status(), StatusCode::FORBIDDEN);

        let missing_certificate = request_json(
            &app,
            "/coordination/v1/generations/renew",
            json!({
                "logical_node": "node-a",
                "generation_id": "green",
                "available_slots": 1,
                "health_expires_at": Utc::now() + Duration::minutes(5),
            }),
            None,
        )
        .await;
        assert_eq!(missing_certificate.status(), StatusCode::UNAUTHORIZED);
    }

    fn test_app() -> Router {
        router(AppState {
            store: Store::open_in_memory().unwrap(),
            token_cipher: Arc::new(TokenCipher::new(&[3_u8; 32], 1).unwrap()),
            certificate_authority: None,
        })
    }

    fn test_coordination_app() -> Router {
        coordination_router(AppState {
            store: Store::open_in_memory().unwrap(),
            token_cipher: Arc::new(TokenCipher::new(&[3_u8; 32], 1).unwrap()),
            certificate_authority: None,
        })
    }

    async fn post_json<T: DeserializeOwned>(app: &Router, uri: &str, body: Value) -> T {
        let identity = if uri.contains("/promote")
            || uri.contains("/drain")
            || uri.contains("/enrollment-tokens")
            || uri.contains("/terminate")
        {
            CoordinationIdentity::Operator
        } else {
            node_identity()
        };
        let response = request_json(app, uri, body, Some(identity)).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        response_json(response).await
    }

    async fn request_json(
        app: &Router,
        uri: &str,
        body: Value,
        identity: Option<CoordinationIdentity>,
    ) -> Response {
        let mut request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        if let Some(identity) = identity {
            request.extensions_mut().insert(identity);
        }
        app.clone().oneshot(request).await.unwrap()
    }

    async fn response_json<T: DeserializeOwned>(response: Response) -> T {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn node_identity() -> CoordinationIdentity {
        CoordinationIdentity::Node {
            logical_node: "node-a".into(),
            generation_id: "green".into(),
        }
    }

    fn test_authority() -> Arc<crate::pki::CertificateAuthority> {
        let root_key = KeyPair::generate().unwrap();
        let mut root_params = CertificateParams::default();
        root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        root_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::CrlSign,
        ];
        let intermediate_key = KeyPair::generate().unwrap();
        let mut intermediate_params = CertificateParams::default();
        intermediate_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        intermediate_params.key_usages = root_params.key_usages.clone();
        let intermediate = intermediate_params
            .signed_by(
                &intermediate_key,
                &Issuer::from_params(&root_params, &root_key),
            )
            .unwrap();
        Arc::new(
            crate::pki::CertificateAuthority::from_pem(
                &intermediate.pem(),
                &intermediate_key.serialize_pem(),
            )
            .unwrap(),
        )
    }
}
