mod error;
mod types;

use std::path::PathBuf;

use chrono::{Duration, Utc};
use reqwest::{Certificate, Client, Identity};
use serde::{de::DeserializeOwned, Serialize};

pub use error::{Error, Result};
pub use types::{ActivationClaim, DesiredPeer, PeerSnapshot, SessionRecord, SessionState};
use types::{
    ApiError, GenerationIdentityRequest, GenerationRegistration, GenerationRenewalRequest,
    PaymentIntent, PaymentIntentRequest, PaymentRedemptionRequest, PeerAcknowledgementRequest,
    SessionClaimRequest, SessionTokenRequest,
};

const API_PREFIX: &str = "/coordination/v1";
const HEALTH_LEASE_SECONDS: i64 = 90;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientConfig {
    pub url: String,
    pub logical_node: String,
    pub generation_id: String,
    pub root_ca_path: PathBuf,
    pub certificate_path: PathBuf,
    pub private_key_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationMetadata {
    pub node_name: String,
    pub region: String,
    pub country_code: Option<String>,
    pub subdivision_code: Option<String>,
    pub city: Option<String>,
    pub api_url: String,
    pub wireguard_endpoint: String,
    pub wireguard_public_key: String,
    pub expected_exit_ip: String,
    pub tunnel_network: String,
}

#[derive(Clone)]
pub struct CoordinatorClient {
    http: Client,
    base_url: String,
    logical_node: String,
    generation_id: String,
}

impl CoordinatorClient {
    pub async fn new(config: &ClientConfig) -> Result<Self> {
        let root = tokio::fs::read(&config.root_ca_path).await?;
        let certificate = tokio::fs::read(&config.certificate_path).await?;
        let private_key = tokio::fs::read(&config.private_key_path).await?;
        let mut identity_pem = certificate;
        identity_pem.push(b'\n');
        identity_pem.extend(private_key);
        let identity = Identity::from_pem(&identity_pem).map_err(|error| {
            Error::Configuration(format!("invalid coordinator client identity: {error}"))
        })?;
        let root = Certificate::from_pem(&root).map_err(|error| {
            Error::Configuration(format!("invalid coordinator root CA: {error}"))
        })?;
        let http = Client::builder()
            .https_only(true)
            .identity(identity)
            .add_root_certificate(root)
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self {
            http,
            base_url: config.url.trim_end_matches('/').to_string(),
            logical_node: config.logical_node.clone(),
            generation_id: config.generation_id.clone(),
        })
    }

    pub async fn register_generation(
        &self,
        metadata: &GenerationMetadata,
        available_slots: u32,
    ) -> Result<()> {
        self.post_ack(
            "/generations/register",
            &GenerationRegistration {
                logical_node: self.logical_node.clone(),
                generation_id: self.generation_id.clone(),
                node_name: metadata.node_name.clone(),
                region: metadata.region.clone(),
                country_code: metadata.country_code.clone(),
                subdivision_code: metadata.subdivision_code.clone(),
                city: metadata.city.clone(),
                api_url: metadata.api_url.clone(),
                wireguard_endpoint: metadata.wireguard_endpoint.clone(),
                wireguard_public_key: metadata.wireguard_public_key.clone(),
                expected_exit_ip: metadata.expected_exit_ip.clone(),
                tunnel_network: metadata.tunnel_network.clone(),
                available_slots,
                health_expires_at: Utc::now() + Duration::seconds(HEALTH_LEASE_SECONDS),
            },
        )
        .await
    }

    pub async fn renew_generation(&self, available_slots: u32) -> Result<()> {
        self.post_ack(
            "/generations/renew",
            &GenerationRenewalRequest {
                logical_node: self.logical_node.clone(),
                generation_id: self.generation_id.clone(),
                available_slots,
                health_expires_at: Utc::now() + Duration::seconds(HEALTH_LEASE_SECONDS),
            },
        )
        .await
    }

    pub async fn create_payment_intent(
        &self,
        intent_id: String,
        duration_seconds: u64,
        request_fingerprint: [u8; 32],
        challenge_key_version: u32,
    ) -> Result<PaymentIntent> {
        self.post(
            "/payment-intents",
            &PaymentIntentRequest {
                intent_id: Some(intent_id),
                logical_node: self.logical_node.clone(),
                duration_seconds,
                request_fingerprint,
                challenge_key_version,
                expires_at: Utc::now() + Duration::minutes(5),
            },
        )
        .await
    }

    pub async fn redeem_payment(
        &self,
        intent_id: String,
        transaction_reference: String,
        request_fingerprint: [u8; 32],
        grace_seconds: u64,
    ) -> Result<SessionRecord> {
        self.post(
            "/payments/redeem",
            &PaymentRedemptionRequest {
                intent_id,
                payment_method: "tempo".into(),
                transaction_reference,
                request_fingerprint,
                grace_seconds,
            },
        )
        .await
    }

    pub async fn status(&self, session_id: String) -> Result<SessionRecord> {
        self.post("/sessions/status", &SessionTokenRequest { session_id })
            .await
    }

    pub async fn heartbeat(&self, session_id: String) -> Result<SessionRecord> {
        self.post("/sessions/heartbeat", &SessionTokenRequest { session_id })
            .await
    }

    pub async fn claim(
        &self,
        session_id: String,
        client_public_key: String,
    ) -> Result<ActivationClaim> {
        self.post(
            "/sessions/claim",
            &SessionClaimRequest {
                session_id,
                client_public_key,
                generation_id: self.generation_id.clone(),
            },
        )
        .await
    }

    pub async fn fail_activation(&self, session_id: String) -> Result<()> {
        self.post_ack(
            "/sessions/activation-failed",
            &SessionTokenRequest { session_id },
        )
        .await
    }

    pub async fn pause(&self, session_id: String) -> Result<SessionRecord> {
        self.post("/sessions/pause", &SessionTokenRequest { session_id })
            .await
    }

    pub async fn peer_snapshot(&self) -> Result<PeerSnapshot> {
        self.post(
            "/peers/snapshot",
            &GenerationIdentityRequest {
                logical_node: self.logical_node.clone(),
                generation_id: self.generation_id.clone(),
            },
        )
        .await
    }

    pub async fn acknowledge_peers(
        &self,
        applied_revision: u64,
        actual_peer_count: u64,
    ) -> Result<()> {
        self.post_ack(
            "/peers/acknowledge",
            &PeerAcknowledgementRequest {
                logical_node: self.logical_node.clone(),
                generation_id: self.generation_id.clone(),
                applied_revision,
                actual_peer_count,
            },
        )
        .await
    }

    async fn post_ack<T: Serialize + ?Sized>(&self, path: &str, request: &T) -> Result<()> {
        let _: serde_json::Value = self.post(path, request).await?;
        Ok(())
    }

    async fn post<TRequest, TResponse>(&self, path: &str, request: &TRequest) -> Result<TResponse>
    where
        TRequest: Serialize + ?Sized,
        TResponse: DeserializeOwned,
    {
        let url = format!("{}{}{}", self.base_url, API_PREFIX, path);
        let response = self
            .http
            .post(url)
            .json(request)
            .send()
            .await
            .map_err(|error| Error::Unavailable(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let message = response
                .json::<ApiError>()
                .await
                .map(|body| body.error)
                .unwrap_or_else(|_| "coordinator request failed".into());
            return Err(Error::Rejected {
                status: status.as_u16(),
                message,
            });
        }
        response
            .json()
            .await
            .map_err(|error| Error::Protocol(error.to_string()))
    }
}
