use std::{collections::BTreeMap, sync::Arc, time::Duration};

use axum::{
    extract::{Query, State},
    http::{header::CONTENT_TYPE, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tower_http::cors::{Any, CorsLayer};

const DEGRADED_HEADER: &str = "x-tempvpn-degraded";

#[derive(Clone, Debug)]
pub struct Upstream {
    pub name: String,
    pub url: String,
}

#[derive(Clone)]
pub struct AppState {
    client: reqwest::Client,
    upstreams: Arc<[Upstream]>,
}

impl AppState {
    pub fn new(upstreams: Vec<Upstream>, timeout: Duration) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder().timeout(timeout).build()?,
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
        .allow_methods([Method::GET, Method::OPTIONS])
        .allow_headers([CONTENT_TYPE])
        .expose_headers([degraded]);

    Router::new()
        .route("/health", get(health))
        .route("/nodes", get(nodes))
        .layer(cors)
        .with_state(state)
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

    let mut response = Json(merged.into_values().collect::<Vec<_>>()).into_response();
    response.headers_mut().insert(
        HeaderName::from_static(DEGRADED_HEADER),
        HeaderValue::from_static(if failed > 0 { "true" } else { "false" }),
    );
    response
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
        body::Body,
        extract::{Query, State},
        http::{header::ACCESS_CONTROL_ALLOW_ORIGIN, Request},
        routing::get,
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
