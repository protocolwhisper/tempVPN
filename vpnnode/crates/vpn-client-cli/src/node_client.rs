use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct NodeClient {
    base_url: String,
    admin_token: String,
    client: Client,
}

#[derive(Debug, Serialize)]
struct CreateSessionRequest<'a> {
    client_public_key: &'a str,
    duration_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    pub session_id: String,
    pub assigned_ip: String,
    pub server_public_key: String,
    pub endpoint: String,
    pub expires_at: DateTime<Utc>,
}

impl NodeClient {
    pub fn new(base_url: String, admin_token: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            admin_token,
            client: Client::new(),
        }
    }

    pub async fn create_session(&self, public_key: &str, duration_seconds: u64) -> Result<Session> {
        let response = self
            .client
            .post(format!("{}/sessions", self.base_url))
            .bearer_auth(&self.admin_token)
            .json(&CreateSessionRequest {
                client_public_key: public_key,
                duration_seconds,
            })
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::HttpStatus {
                operation: "session create",
                status,
                body,
            });
        }

        Ok(response.json().await?)
    }

    pub async fn revoke_session(&self, session_id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!("{}/sessions/{session_id}", self.base_url))
            .bearer_auth(&self.admin_token)
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::HttpStatus {
                operation: "session revoke",
                status,
                body,
            });
        }
        Ok(())
    }
}
