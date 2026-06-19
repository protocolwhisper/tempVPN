use std::net::SocketAddr;

use reqwest::{Client, Proxy};
use tokio::net::TcpStream;

use crate::{
    error::{Error, Result},
    wireguard_client::WireGuardTunnel,
};

pub async fn check_proxy(addr: SocketAddr) -> Result<()> {
    TcpStream::connect(addr).await.map_err(Error::Io)?;
    Ok(())
}

pub async fn check_tunnel(tunnel: &WireGuardTunnel) -> Result<()> {
    if tunnel.is_active().await {
        Ok(())
    } else {
        Err(Error::TunnelInactive(tunnel.interface_name().to_string()))
    }
}

pub async fn visible_ip(proxy_addr: SocketAddr) -> Result<String> {
    let proxy = Proxy::all(format!("socks5h://{proxy_addr}"))?;
    let client = Client::builder().proxy(proxy).build()?;
    let response = client.get("https://ifconfig.me/ip").send().await?;
    if !response.status().is_success() {
        return Err(Error::ExitIpCheckStatus(response.status()));
    }
    Ok(response.text().await?.trim().to_string())
}
