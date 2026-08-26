use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
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

#[derive(Clone, Debug, Serialize)]
pub struct GenerationRenewalRequest {
    pub logical_node: String,
    pub generation_id: String,
    pub available_slots: u32,
    pub health_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GenerationIdentityRequest {
    pub logical_node: String,
    pub generation_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaymentIntentRequest {
    pub intent_id: Option<String>,
    pub logical_node: String,
    pub duration_seconds: u64,
    pub request_fingerprint: [u8; 32],
    pub challenge_key_version: u32,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PaymentIntent {
    pub intent_id: String,
    pub logical_node: String,
    pub duration_seconds: u64,
    pub request_fingerprint: Vec<u8>,
    pub challenge_key_version: u32,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PaymentRedemptionRequest {
    pub intent_id: String,
    pub payment_method: String,
    pub transaction_reference: String,
    pub request_fingerprint: [u8; 32],
    pub grace_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionTokenRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionClaimRequest {
    pub session_id: String,
    pub client_public_key: String,
    pub generation_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Paused,
    Active,
    Expired,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Deserialize)]
pub struct ActivationClaim {
    pub session: SessionRecord,
    pub generation_id: String,
    pub wireguard_endpoint: String,
    pub wireguard_public_key: String,
    pub expected_exit_ip: String,
    pub desired_revision: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct DesiredPeer {
    pub session_id: String,
    pub client_public_key: String,
    pub assigned_ip: String,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PeerSnapshot {
    pub logical_node: String,
    pub generation_id: String,
    pub revision: u64,
    pub peers: Vec<DesiredPeer>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PeerAcknowledgementRequest {
    pub logical_node: String,
    pub generation_id: String,
    pub applied_revision: u64,
    pub actual_peer_count: u64,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub error: String,
}
