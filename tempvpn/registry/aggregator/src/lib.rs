use std::{collections::BTreeMap, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{
        header::{ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, WWW_AUTHENTICATE},
        HeaderMap, HeaderName, HeaderValue, Method, StatusCode,
    },
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use futures::{future::join_all, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tower_http::cors::{Any, CorsLayer};

const DEGRADED_HEADER: &str = "x-tempvpn-degraded";
const NODE_ID_HEADER: &str = "x-tempvpn-node-id";
const PAYMENT_RECEIPT_HEADER: &str = "payment-receipt";
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
        })
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
        .route("/docs/markdown.md", get(service_docs_markdown))
        .route("/llms.txt", get(llms_txt))
        .route("/health", get(health))
        .route("/nodes", get(nodes))
        .route("/sessions", post(create_session))
        .route("/sessions/{session_id}/connect", post(connect_session))
        .route("/sessions/{session_id}/pause", post(pause_session))
        .route("/sessions/{session_id}/heartbeat", post(heartbeat_session))
        .route("/sessions/{session_id}/status", get(session_status))
        .route("/sessions/stream", post(stream_session))
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
        "docs_markdown": "/docs/markdown.md",
        "openapi": "/openapi.json",
        "llms": "/llms.txt"
    }))
}

async fn service_docs() -> Redirect {
    Redirect::permanent("https://tempvpn.xyz/docs/")
}

async fn service_docs_markdown() -> Response {
    let document = "# TempVPN\n\nTempVPN sells temporary WireGuard VPN access through Tempo MPP on mainnet. The registry is the control-plane origin for discovery, payment, and session lifecycle requests.\n\n## Agent workflow\n\n1. Discover a node with `GET /nodes?available=true` and retain its `id`.\n2. Buy fixed access with `POST /sessions` and JSON `{\"node_id\":\"madrid\",\"duration_seconds\":1800}`.\n3. Connect it with `POST /sessions/{session_id}/connect` and the selected `node_id` plus a locally generated WireGuard public key.\n4. Use `GET /sessions/{session_id}/status`, `POST /sessions/{session_id}/heartbeat`, and `POST /sessions/{session_id}/pause` throughout the fixed session. A paused balance can reconnect through another available node.\n5. Use `POST /sessions/stream` as a separate, node-bound metered product; streaming credentials are not portable fixed-session balances.\n\nDuration is measured in 60-second billing intervals. Treat the runtime HTTP 402 challenge as authoritative for the price and payment terms. Node `api_url` values are diagnostic metadata, not client payment origins.\n\nMachine-readable API: /openapi.json\n";
    ([(CONTENT_TYPE, "text/plain; charset=utf-8")], document).into_response()
}

async fn llms_txt() -> Response {
    let document = "# TempVPN\n\n> Buy temporary WireGuard VPN access with Tempo MPP.\n\nService: https://registry.tempvpn.xyz\nOpenAPI: https://registry.tempvpn.xyz/openapi.json\nDocs: https://tempvpn.xyz/docs/\nMarkdown docs: https://registry.tempvpn.xyz/docs/markdown.md\n\nUse the registry origin for every request below. Node api_url values are diagnostic metadata, not payment origins.\nDiscover nodes: GET /nodes?available=true; retain the selected node id.\nFixed purchase: POST /sessions JSON {\"node_id\": \"madrid\", \"duration_seconds\": 1800}\nConnect fixed session: POST /sessions/<session_id>/connect JSON {\"node_id\": \"madrid\", \"client_public_key\": \"<wireguard-public-key>\"}\nStatus: GET /sessions/<session_id>/status\nHeartbeat: POST /sessions/<session_id>/heartbeat\nPause: POST /sessions/<session_id>/pause\nStreaming: POST /sessions/stream JSON {\"node_id\": \"madrid\", \"client_public_key\": \"<wireguard-public-key>\", \"duration_seconds\": 1800}\nStreaming is a separate node-bound metered product and is not a portable fixed-session balance.\nBilling interval: 60 seconds. Payment: Tempo mainnet MPP; follow the runtime 402 challenge for authoritative price and terms.\nNever send a wallet private key or WireGuard private key to TempVPN.\n";
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
    proxy_to_selected_node(&state, headers, body, "/sessions").await
}

async fn connect_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    proxy_to_selected_node(
        &state,
        headers,
        body,
        &format!("/sessions/{session_id}/connect"),
    )
    .await
}

async fn stream_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    proxy_to_selected_node(&state, headers, body, "/sessions/stream").await
}

async fn pause_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
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
    proxy_to_any_node(
        &state,
        headers,
        Bytes::new(),
        Method::GET,
        &format!("/sessions/{session_id}/status"),
    )
    .await
}

async fn proxy_to_selected_node(
    state: &AppState,
    headers: HeaderMap,
    body: Bytes,
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
    let nodes = merge_nodes(fetch_all(state, &NodesQuery::default()).await);
    let Some(node) = nodes.get(&node_id) else {
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
    proxy_request(state, headers, body, Method::POST, api_url, path).await
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
    for name in [AUTHORIZATION, ACCEPT, CONTENT_TYPE] {
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
    use serde_json::{json, Value};
    use tokio::{net::TcpListener, sync::Mutex};
    use tower::ServiceExt;

    use super::*;

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
            .contains("60-second billing intervals"));

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
        assert_eq!(
            document["components"]["parameters"]["SessionId"]["required"],
            true
        );

        let session = &document["components"]["schemas"]["Session"];
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
                    .uri("/docs/markdown.md")
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
