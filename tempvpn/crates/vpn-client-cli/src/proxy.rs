use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use tracing::{debug, warn};

use crate::error::{Error, Result};

pub struct ProxyHandle {
    pub addr: SocketAddr,
    healthy: Arc<AtomicBool>,
    shutdown: Option<oneshot::Sender<()>>,
    join: JoinHandle<()>,
}

impl ProxyHandle {
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    pub async fn stop(mut self) {
        self.healthy.store(false, Ordering::SeqCst);
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = self.join.await;
    }
}

pub async fn start(addr: SocketAddr) -> Result<ProxyHandle> {
    if !addr.ip().is_loopback() {
        return Err(Error::ProxyMustBeLoopback(addr));
    }

    let listener = TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    let healthy = Arc::new(AtomicBool::new(true));
    let task_healthy = healthy.clone();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    task_healthy.store(false, Ordering::SeqCst);
                    break;
                }
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, peer)) => {
                            tokio::spawn(async move {
                                if let Err(err) = handle_client(stream).await {
                                    debug!(%peer, error = %err, "SOCKS5 client failed");
                                }
                            });
                        }
                        Err(err) => {
                            task_healthy.store(false, Ordering::SeqCst);
                            warn!(error = %err, "SOCKS5 accept failed");
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(ProxyHandle {
        addr,
        healthy,
        shutdown: Some(shutdown_tx),
        join,
    })
}

async fn handle_client(mut inbound: TcpStream) -> Result<()> {
    let version = inbound.read_u8().await?;
    if version != 0x05 {
        return Err(Error::UnsupportedSocksVersion(version));
    }
    let method_count = inbound.read_u8().await? as usize;
    let mut methods = vec![0u8; method_count];
    inbound.read_exact(&mut methods).await?;
    if !methods.contains(&0x00) {
        inbound.write_all(&[0x05, 0xff]).await?;
        return Err(Error::SocksNoAuthUnavailable);
    }
    inbound.write_all(&[0x05, 0x00]).await?;

    let version = inbound.read_u8().await?;
    let command = inbound.read_u8().await?;
    let _reserved = inbound.read_u8().await?;
    let atyp = inbound.read_u8().await?;
    if version != 0x05 || command != 0x01 {
        write_failure(&mut inbound, 0x07).await?;
        return Err(Error::SocksConnectOnly);
    }

    let target = read_target(&mut inbound, atyp).await?;
    let port = inbound.read_u16().await?;
    let outbound = connect_target(&target, port).await;

    let mut outbound = match outbound {
        Ok(outbound) => outbound,
        Err(err) => {
            write_failure(&mut inbound, 0x05).await?;
            return Err(err);
        }
    };

    inbound
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    io::copy_bidirectional(&mut inbound, &mut outbound).await?;
    Ok(())
}

enum Target {
    Domain(String),
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

async fn read_target(inbound: &mut TcpStream, atyp: u8) -> Result<Target> {
    match atyp {
        0x01 => {
            let mut raw = [0u8; 4];
            inbound.read_exact(&mut raw).await?;
            Ok(Target::V4(Ipv4Addr::from(raw)))
        }
        0x03 => {
            let len = inbound.read_u8().await? as usize;
            let mut raw = vec![0u8; len];
            inbound.read_exact(&mut raw).await?;
            let domain = String::from_utf8(raw).map_err(Error::InvalidSocksDomain)?;
            Ok(Target::Domain(domain))
        }
        0x04 => {
            let mut raw = [0u8; 16];
            inbound.read_exact(&mut raw).await?;
            Ok(Target::V6(Ipv6Addr::from(raw)))
        }
        other => Err(Error::UnsupportedSocksAddressType(other)),
    }
}

async fn connect_target(target: &Target, port: u16) -> Result<TcpStream> {
    match target {
        Target::Domain(domain) => Ok(TcpStream::connect((domain.as_str(), port)).await?),
        Target::V4(addr) => Ok(TcpStream::connect(SocketAddr::from((*addr, port))).await?),
        Target::V6(addr) => Ok(TcpStream::connect(SocketAddr::from((*addr, port))).await?),
    }
}

async fn write_failure(inbound: &mut TcpStream, code: u8) -> Result<()> {
    inbound
        .write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    Ok(())
}
