use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use mpp::server::axum::{ChargeChallenger, ChargeConfig, MppCharge};
use mpp::server::{tempo, Mpp, TempoConfig};
use serde_json::{json, Value};
use std::{
    env,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::net::TcpListener;

const DEFAULT_PORT: u16 = 3000;
const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_REALM: &str = "localhost:3000";
const DEFAULT_RPC_URL: &str = "https://rpc.moderato.tempo.xyz";
const DEFAULT_CURRENCY: &str = "0x20c0000000000000000000000000000000000000";
const PRICE_AMOUNT: &str = "0.01";

#[derive(Clone)]
struct ServiceConfig {
    port: u16,
    host: IpAddr,
    realm: String,
    rpc_url: String,
    currency: String,
    recipient: String,
    public_base_url: String,
}

struct OneCent;

impl ChargeConfig for OneCent {
    fn amount() -> &'static str {
        PRICE_AMOUNT
    }

    fn description() -> Option<&'static str> {
        Some("Paid response from the Rust Tempo MPP payment service")
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = ServiceConfig::from_env()?;
    let mpp = Mpp::create(
        tempo(TempoConfig {
            recipient: config.recipient.as_str(),
        })
        .currency(config.currency.as_str())
        .rpc_url(config.rpc_url.as_str())
        .realm(config.realm.as_str()),
    )?;

    let state = AppState {
        config: Arc::new(config.clone()),
        challenger: Arc::new(mpp) as Arc<dyn ChargeChallenger>,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/free", get(free))
        .route("/paid/time", get(paid_time))
        .route("/openapi.json", get(openapi))
        .route("/.well-known/mpp/openapi.json", get(openapi))
        .with_state(state);

    let addr = SocketAddr::from((config.host, config.port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(
        "MPP payment service listening on {}",
        config.public_base_url
    );
    tracing::info!(
        "Paid endpoints require {} pathUSD on Tempo Moderato",
        PRICE_AMOUNT
    );

    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Clone)]
struct AppState {
    config: Arc<ServiceConfig>,
    challenger: Arc<dyn ChargeChallenger>,
}

impl axum::extract::FromRef<AppState> for Arc<dyn ChargeChallenger> {
    fn from_ref(state: &AppState) -> Self {
        state.challenger.clone()
    }
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "tempo-mpp-payment-service",
        "implementation": "rust",
        "chainId": 42431,
        "payment": {
            "method": "tempo",
            "intent": "charge",
            "currency": state.config.currency,
            "amount": PRICE_AMOUNT
        }
    }))
}

async fn free() -> Json<Value> {
    Json(json!({
        "message": "This endpoint is free. Use /paid/time to trigger an MPP 402 flow."
    }))
}

async fn paid_time(charge: MppCharge<OneCent>) -> impl IntoResponse {
    let access_key = uuid::Uuid::new_v4().to_string();

    Json(json!({
        "paid": true,
        "accessKey": access_key,
        "reference": charge.receipt.reference,
        "now": chrono_like_timestamp(),
        "chainId": 42431
    }))
}

async fn openapi(State(state): State<AppState>) -> Json<Value> {
    let offer = json!({
        "amount": PRICE_AMOUNT,
        "currency": state.config.currency,
        "description": "One-time Tempo testnet charge",
        "intent": "charge",
        "method": "tempo"
    });

    Json(json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Rust Tempo MPP Payment Service",
            "version": "0.1.0",
            "description": "Minimal Rust MPP-enabled API using Tempo one-time charge payments."
        },
        "servers": [{ "url": state.config.public_base_url }],
        "x-service-info": {
            "categories": ["payments", "tempo", "mpp", "rust"],
            "docs": {
                "homepage": state.config.public_base_url,
                "apiReference": format!("{}/openapi.json", state.config.public_base_url),
                "llms": "https://mpp.dev/llms-full.txt"
            }
        },
        "paths": {
            "/health": {
                "get": {
                    "summary": "Health check",
                    "responses": { "200": { "description": "Service status" } }
                }
            },
            "/free": {
                "get": {
                    "summary": "Free endpoint",
                    "responses": { "200": { "description": "Free response" } }
                }
            },
            "/paid/time": {
                "get": {
                    "summary": "Paid current time endpoint",
                    "x-payment-info": { "offers": [offer] },
                    "responses": {
                        "200": { "description": "Paid response" },
                        "402": { "description": "Payment Required" }
                    }
                }
            }
        }
    }))
}

impl ServiceConfig {
    fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let port = env::var("PORT")
            .ok()
            .map(|value| value.parse::<u16>())
            .transpose()?
            .unwrap_or(DEFAULT_PORT);
        let host = env::var("HOST")
            .unwrap_or_else(|_| DEFAULT_HOST.to_string())
            .parse::<IpAddr>()?;
        let realm = env::var("MPP_REALM").unwrap_or_else(|_| DEFAULT_REALM.to_string());
        let rpc_url = env::var("TEMPO_RPC_URL").unwrap_or_else(|_| DEFAULT_RPC_URL.to_string());
        let currency =
            env::var("MPP_PAYMENT_CURRENCY").unwrap_or_else(|_| DEFAULT_CURRENCY.to_string());
        let recipient = required_env("MPP_PAYMENT_RECIPIENT")?;
        let public_base_url =
            env::var("MPP_PUBLIC_BASE_URL").unwrap_or_else(|_| format!("http://localhost:{port}"));

        Ok(Self {
            port,
            host,
            realm,
            rpc_url,
            currency,
            recipient,
            public_base_url,
        })
    }
}

fn required_env(name: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    env::var(name).map_err(|_| format!("{name} is required").into())
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{seconds}")
}
