use tokio::process::Command;
use tracing::info;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct WireGuard {
    command: String,
    interface: String,
    mock: bool,
}

impl WireGuard {
    pub fn new(command: String, interface: String, mock: bool) -> Self {
        Self {
            command,
            interface,
            mock,
        }
    }

    pub async fn add_peer(&self, public_key: &str, allowed_ip: &str) -> Result<()> {
        if self.mock {
            info!(public_key, allowed_ip, "mock wg add peer");
            return Ok(());
        }

        let output = Command::new(&self.command)
            .args([
                "set",
                &self.interface,
                "peer",
                public_key,
                "allowed-ips",
                allowed_ip,
            ])
            .output()
            .await
            .map_err(Error::Io)?;

        if !output.status.success() {
            return Err(Error::CommandFailed {
                program: format!(
                    "{} set {} peer <key> allowed-ips",
                    self.command, self.interface
                ),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        info!(public_key, allowed_ip, "added WireGuard peer");
        Ok(())
    }

    pub async fn remove_peer(&self, public_key: &str) -> Result<()> {
        if self.mock {
            info!(public_key, "mock wg remove peer");
            return Ok(());
        }

        let output = Command::new(&self.command)
            .args(["set", &self.interface, "peer", public_key, "remove"])
            .output()
            .await
            .map_err(Error::Io)?;

        if !output.status.success() {
            return Err(Error::CommandFailed {
                program: format!("{} set {} peer <key> remove", self.command, self.interface),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        info!(public_key, "removed WireGuard peer");
        Ok(())
    }
}
