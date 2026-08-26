use tokio::process::Command;
use tracing::info;

#[cfg(test)]
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
#[cfg(test)]
use tokio::sync::Mutex;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct WireGuard {
    command: String,
    interface: String,
    mock: bool,
    #[cfg(test)]
    mock_peers: Arc<Mutex<HashSet<String>>>,
    #[cfg(test)]
    mock_remove_failures: Arc<AtomicUsize>,
}

impl WireGuard {
    pub fn new(command: String, interface: String, mock: bool) -> Self {
        Self {
            command,
            interface,
            mock,
            #[cfg(test)]
            mock_peers: Arc::new(Mutex::new(HashSet::new())),
            #[cfg(test)]
            mock_remove_failures: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub async fn add_peer(&self, public_key: &str, allowed_ip: &str) -> Result<()> {
        if self.mock {
            #[cfg(test)]
            self.mock_peers.lock().await.insert(public_key.to_string());
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
            #[cfg(test)]
            {
                if self
                    .mock_remove_failures
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                        remaining.checked_sub(1)
                    })
                    .is_ok()
                {
                    return Err(Error::CommandFailed {
                        program: "mock wg remove peer".into(),
                        stderr: "injected mock failure".into(),
                    });
                }
                self.mock_peers.lock().await.remove(public_key);
            }
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

    #[cfg(test)]
    pub async fn mock_has_peer(&self, public_key: &str) -> bool {
        self.mock_peers.lock().await.contains(public_key)
    }

    #[cfg(test)]
    pub fn mock_fail_next_removals(&self, count: usize) {
        self.mock_remove_failures.store(count, Ordering::SeqCst);
    }
}
