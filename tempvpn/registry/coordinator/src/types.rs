use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{crypto::TokenCipher, pki::CertificateAuthority, store::Store};

pub const COORDINATION_API_V1: &str = "/coordination/v1";

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub token_cipher: Arc<TokenCipher>,
    pub certificate_authority: Option<Arc<CertificateAuthority>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct NodeRecord {
    pub id: String,
    pub name: String,
    pub region: String,
    pub country_code: Option<String>,
    pub subdivision_code: Option<String>,
    pub city: Option<String>,
    pub api_url: String,
    pub wireguard_endpoint: String,
    pub wireguard_public_key: String,
    pub expected_exit_ip: String,
    pub accepting_sessions: bool,
    pub available_slots: u32,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionState {
    Standby,
    Accepting,
    Draining,
    Retired,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GenerationRegistration {
    pub logical_node: String,
    pub generation_id: String,
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
    pub available_slots: u32,
    pub health_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DrainStatus {
    pub logical_node: String,
    pub generation_id: String,
    pub admission_state: AdmissionState,
    pub active_sessions: u64,
    pub transitional_sessions: u64,
    pub desired_peer_revision: u64,
    pub applied_peer_revision: u64,
    pub actual_peer_count: u64,
    pub safe_to_delete: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PaymentIntent {
    pub intent_id: String,
    pub logical_node: String,
    pub duration_seconds: u64,
    pub request_fingerprint: Vec<u8>,
    pub challenge_key_version: u32,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Paused,
    Active,
    Expired,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionRecord {
    pub session_id: String,
    pub logical_node: String,
    pub node_url: String,
    pub state: SessionState,
    pub phase: Option<String>,
    pub total_seconds: u64,
    pub remaining_seconds: u64,
    pub created_at: DateTime<Utc>,
    pub connected_at: Option<DateTime<Utc>>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub grace_deadline: DateTime<Utc>,
    pub assigned_ip: Option<String>,
    pub client_public_key: Option<String>,
    pub active_generation_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ActivationClaim {
    pub session: SessionRecord,
    pub generation_id: String,
    pub wireguard_endpoint: String,
    pub wireguard_public_key: String,
    pub expected_exit_ip: String,
    pub desired_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DesiredPeer {
    pub session_id: String,
    pub client_public_key: String,
    pub assigned_ip: String,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PeerSnapshot {
    pub logical_node: String,
    pub generation_id: String,
    pub revision: u64,
    pub peers: Vec<DesiredPeer>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ApiAcknowledgement {
    pub ok: bool,
}

impl ApiAcknowledgement {
    pub fn ok() -> Self {
        Self { ok: true }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GenerationRenewalRequest {
    pub logical_node: String,
    pub generation_id: String,
    pub available_slots: u32,
    pub health_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GenerationIdentityRequest {
    pub logical_node: String,
    pub generation_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PaymentIntentRequest {
    pub intent_id: Option<String>,
    pub logical_node: String,
    pub duration_seconds: u64,
    pub request_fingerprint: [u8; 32],
    pub challenge_key_version: u32,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PaymentRedemptionRequest {
    pub intent_id: String,
    pub payment_method: String,
    pub transaction_reference: String,
    pub request_fingerprint: [u8; 32],
    pub grace_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionTokenRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionClaimRequest {
    pub session_id: String,
    pub client_public_key: String,
    pub generation_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PeerAcknowledgementRequest {
    pub logical_node: String,
    pub generation_id: String,
    pub applied_revision: u64,
    pub actual_peer_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct EnrollmentRequest {
    pub enrollment_token: String,
    pub logical_node: String,
    pub generation_id: String,
    pub certificate_signing_request_pem: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CertificateRenewalRequest {
    pub certificate_signing_request_pem: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CertificateResponse {
    pub certificate_chain_pem: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum CoordinationIdentity {
    Node {
        logical_node: String,
        generation_id: String,
    },
    Operator,
}

impl CoordinationIdentity {
    pub fn node_scope(&self) -> crate::Result<(&str, &str)> {
        match self {
            Self::Node {
                logical_node,
                generation_id,
            } => Ok((logical_node, generation_id)),
            Self::Operator => Err(crate::Error::Forbidden),
        }
    }

    pub fn require_node(&self, logical_node: &str, generation_id: &str) -> crate::Result<()> {
        match self {
            Self::Node {
                logical_node: authenticated_node,
                generation_id: authenticated_generation,
            } if authenticated_node == logical_node
                && authenticated_generation == generation_id =>
            {
                Ok(())
            }
            Self::Node { .. } | Self::Operator => Err(crate::Error::Forbidden),
        }
    }

    pub fn require_logical_node(&self, logical_node: &str) -> crate::Result<()> {
        match self {
            Self::Node {
                logical_node: authenticated_node,
                ..
            } if authenticated_node == logical_node => Ok(()),
            Self::Node { .. } | Self::Operator => Err(crate::Error::Forbidden),
        }
    }

    pub fn require_operator(&self) -> crate::Result<()> {
        match self {
            Self::Operator => Ok(()),
            Self::Node { .. } => Err(crate::Error::Forbidden),
        }
    }

    pub fn san_uri(&self) -> String {
        match self {
            Self::Node {
                logical_node,
                generation_id,
            } => format!("spiffe://tempvpn/node/{logical_node}/{generation_id}"),
            Self::Operator => "spiffe://tempvpn/operator".to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct EnrollmentTokenRequest {
    pub logical_node: String,
    pub generation_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct EnrollmentTokenResponse {
    pub enrollment_token: String,
    pub expires_at: DateTime<Utc>,
}
