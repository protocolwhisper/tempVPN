use std::{net::SocketAddr, process::Stdio};

use tokio::{
    process::Command,
    time::{interval, Duration},
};
use tracing::{info, warn};

use crate::{
    error::{Error, Result},
    health,
    proxy::ProxyHandle,
    wireguard_client::WireGuardTunnel,
};

pub enum RunOutcome {
    Exited(i32),
    StoppedByKillSwitch(String),
    Interrupted,
}

pub async fn run_child_with_kill_switch(
    command: &[String],
    proxy_addr: SocketAddr,
    proxy: &ProxyHandle,
    tunnel: &WireGuardTunnel,
) -> Result<RunOutcome> {
    if command.is_empty() {
        return Err(Error::MissingCommand);
    }

    let proxy_url = format!("socks5h://{proxy_addr}");
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .env("HTTP_PROXY", &proxy_url)
        .env("HTTPS_PROXY", &proxy_url)
        .env("ALL_PROXY", &proxy_url)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(Error::Io)?;

    info!(command = ?command, "child process started");
    let mut ticker = interval(Duration::from_secs(2));

    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(Error::Io)?;
                let code = status.code().unwrap_or(1);
                return Ok(RunOutcome::Exited(code));
            }
            _ = tokio::signal::ctrl_c() => {
                warn!("Ctrl+C received, stopping child process");
                stop_child(&mut child).await;
                return Ok(RunOutcome::Interrupted);
            }
            _ = ticker.tick() => {
                if !proxy.is_healthy() || health::check_proxy(proxy_addr).await.is_err() {
                    stop_child(&mut child).await;
                    return Ok(RunOutcome::StoppedByKillSwitch("SOCKS5 proxy is not healthy".to_string()));
                }
                if !tunnel.is_active().await {
                    stop_child(&mut child).await;
                    return Ok(RunOutcome::StoppedByKillSwitch("WireGuard tunnel is not active".to_string()));
                }
            }
        }
    }
}

async fn stop_child(child: &mut tokio::process::Child) {
    if child.id().is_some() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}
