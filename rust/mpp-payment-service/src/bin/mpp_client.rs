use mpp::client::{Fetch, TempoProvider};
use mpp::{PrivateKeySigner, PAYMENT_RECEIPT_HEADER};
use std::env;

const DEFAULT_TARGET_URL: &str = "http://localhost:3000/paid/time";
const DEFAULT_RPC_URL: &str = "https://rpc.moderato.tempo.xyz";
const DEFAULT_CLIENT_ID: &str = "tempo-rust-mpp-client";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = ClientConfig::from_env()?;
    let signer: PrivateKeySigner = config.private_key.parse()?;
    let wallet = signer.address();
    let provider = TempoProvider::new(signer, config.rpc_url.as_str())?
        .with_client_id(config.client_id.as_str());

    let response = reqwest::Client::new()
        .get(config.target_url.as_str())
        .send_with_payment(&provider)
        .await?;

    let status = response.status();
    let receipt = response
        .headers()
        .get(PAYMENT_RECEIPT_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response.text().await?;

    println!("wallet: {wallet}");
    println!("status: {status}");
    if let Some(receipt) = receipt {
        println!("payment-receipt: {receipt}");
    }
    println!("body:");
    println!("{body}");

    Ok(())
}

struct ClientConfig {
    target_url: String,
    rpc_url: String,
    client_id: String,
    private_key: String,
}

impl ClientConfig {
    fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let target_url = env::args()
            .nth(1)
            .unwrap_or_else(|| DEFAULT_TARGET_URL.to_string());
        let rpc_url = env::var("TEMPO_RPC_URL").unwrap_or_else(|_| DEFAULT_RPC_URL.to_string());
        let client_id = env::var("MPP_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
        let private_key = required_env("TEMPO_PRIVATE_KEY")?;

        Ok(Self {
            target_url,
            rpc_url,
            client_id,
            private_key,
        })
    }
}

fn required_env(name: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    env::var(name).map_err(|_| format!("{name} is required").into())
}
