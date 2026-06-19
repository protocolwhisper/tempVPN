use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusFile {
    pub session_id: String,
    pub region: String,
    pub proxy: SocketAddr,
    pub tunnel_ip: String,
    pub exit_ip: Option<String>,
    pub interface_name: String,
    pub config_path: Option<PathBuf>,
    pub expires_at: DateTime<Utc>,
}

impl StatusFile {
    pub async fn write(&self, path: &Path) -> Result<()> {
        let contents = serde_json::to_string_pretty(self)?;
        Ok(tokio::fs::write(path, contents).await?)
    }
}

pub async fn read(path: &Path) -> Result<StatusFile> {
    let contents = tokio::fs::read_to_string(path).await.map_err(Error::Io)?;
    Ok(serde_json::from_str(&contents)?)
}

pub async fn remove(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
}
