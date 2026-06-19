use std::path::{Path, PathBuf};

use tempfile::TempDir;
use tokio::{fs, process::Command};
use tracing::info;

use crate::{
    error::{Error, Result},
    keygen::Keypair,
    node_client::Session,
};

pub struct WireGuardTunnel {
    wg_quick_command: String,
    wg_command: String,
    interface_name: String,
    config_path: PathBuf,
    _temp_dir: TempDir,
}

impl WireGuardTunnel {
    pub async fn up(
        wg_quick_command: String,
        wg_command: String,
        interface_name: String,
        keypair: &Keypair,
        session: &Session,
    ) -> Result<Self> {
        let temp_dir = tempfile::Builder::new()
            .prefix("agent-egress-")
            .tempdir()
            .map_err(Error::Io)?;
        let config_path = temp_dir.path().join(format!("{interface_name}.conf"));
        write_config(&config_path, keypair, session).await?;

        let output = Command::new(&wg_quick_command)
            .arg("up")
            .arg(&config_path)
            .output()
            .await
            .map_err(Error::Io)?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                program: format!("{wg_quick_command} up {}", config_path.display()),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        info!(interface = interface_name, "WireGuard tunnel is up");

        Ok(Self {
            wg_quick_command,
            wg_command,
            interface_name,
            config_path,
            _temp_dir: temp_dir,
        })
    }

    pub async fn is_active(&self) -> bool {
        Command::new(&self.wg_command)
            .args(["show", &self.interface_name])
            .output()
            .await
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub async fn down(&self) -> Result<()> {
        let output = Command::new(&self.wg_quick_command)
            .arg("down")
            .arg(&self.config_path)
            .output()
            .await
            .map_err(Error::Io)?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                program: format!(
                    "{} down {}",
                    self.wg_quick_command,
                    self.config_path.display()
                ),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        info!(interface = self.interface_name, "WireGuard tunnel is down");
        Ok(())
    }

    pub fn interface_name(&self) -> &str {
        &self.interface_name
    }
}

async fn write_config(path: &Path, keypair: &Keypair, session: &Session) -> Result<()> {
    let config = format!(
        "\
[Interface]
PrivateKey = {}
Address = {}

[Peer]
PublicKey = {}
Endpoint = {}
AllowedIPs = 0.0.0.0/0, ::/0
PersistentKeepalive = 25
",
        keypair.private_key, session.assigned_ip, session.server_public_key, session.endpoint
    );
    Ok(fs::write(path, config).await?)
}
