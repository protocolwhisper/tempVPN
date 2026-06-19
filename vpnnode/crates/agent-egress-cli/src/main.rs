mod cli;
mod config;
mod error;
mod health;
mod keygen;
mod node_client;
mod process;
mod proxy;
mod status;
mod wireguard_client;

use chrono::Utc;
use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    cli::{Cli, Command},
    config::Config,
    error::{Error, Result},
    node_client::NodeClient,
    process::{run_child_with_kill_switch, RunOutcome},
    status::StatusFile,
    wireguard_client::WireGuardTunnel,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agent_egress=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    let config = Config::load(cli.config).await?;

    match cli.command {
        Command::Run(args) => run(config, args).await,
        Command::Status => print_status(config).await,
    }
}

async fn run(config: Config, args: cli::RunArgs) -> Result<()> {
    if args.region != "us" {
        return Err(Error::UnsupportedRegion);
    }

    let node = NodeClient::new(config.node_url.clone(), config.admin_token.clone());
    let keypair = keygen::generate(&config.wg_command).await?;
    info!("generated ephemeral WireGuard keypair");

    let session = node
        .create_session(&keypair.public_key, args.duration)
        .await?;
    info!(
        session_id = session.session_id,
        assigned_ip = session.assigned_ip,
        endpoint = session.endpoint,
        "created VPN session"
    );

    let mut tunnel: Option<WireGuardTunnel> = None;
    let mut socks_proxy: Option<proxy::ProxyHandle> = None;
    let mut exit_ip: Option<String> = None;
    let mut child_code = 0;

    let result = async {
        let wg = WireGuardTunnel::up(
            config.wg_quick_command.clone(),
            config.wg_command.clone(),
            config.interface_name.clone(),
            &keypair,
            &session,
        )
        .await?;
        tunnel = Some(wg);
        health::check_tunnel(tunnel.as_ref().expect("tunnel set")).await?;

        let proxy = proxy::start(config.proxy_addr).await?;
        let proxy_addr = proxy.addr;
        socks_proxy = Some(proxy);
        health::check_proxy(proxy_addr).await?;
        info!(addr = %proxy_addr, "SOCKS5 proxy is listening");

        let expected_exit_ip = config
            .expected_exit_ip
            .clone()
            .or_else(|| endpoint_host_ip(&session.endpoint));
        let observed_exit_ip = health::visible_ip(proxy_addr).await?;
        if let Some(expected) = &expected_exit_ip {
            if observed_exit_ip != *expected {
                return Err(Error::ExitIpMismatch {
                    expected: expected.clone(),
                    observed: observed_exit_ip,
                });
            }
        }
        info!(
            exit_ip = observed_exit_ip,
            "verified egress IP through proxy"
        );
        exit_ip = Some(observed_exit_ip.clone());

        StatusFile {
            session_id: session.session_id.clone(),
            region: args.region.clone(),
            proxy: proxy_addr,
            tunnel_ip: session.assigned_ip.clone(),
            exit_ip: Some(observed_exit_ip),
            interface_name: config.interface_name.clone(),
            expires_at: session.expires_at,
        }
        .write(&config.status_file)
        .await?;

        let outcome = run_child_with_kill_switch(
            &args.command,
            proxy_addr,
            socks_proxy.as_ref().expect("proxy set"),
            tunnel.as_ref().expect("tunnel set"),
        )
        .await?;

        match outcome {
            RunOutcome::Exited(code) => {
                child_code = code;
                info!(code, "child process exited");
            }
            RunOutcome::StoppedByKillSwitch(reason) => {
                child_code = 1;
                warn!(reason, "kill-switch stopped child process");
            }
            RunOutcome::Interrupted => {
                child_code = 130;
                warn!("run interrupted");
            }
        }

        Ok::<(), Error>(())
    }
    .await;

    status::remove(&config.status_file).await;

    if let Some(proxy) = socks_proxy {
        proxy.stop().await;
    }
    if let Some(wg) = tunnel {
        if let Err(err) = wg.down().await {
            warn!(error = %err, "failed to bring WireGuard tunnel down");
        }
    }
    if let Err(err) = node.revoke_session(&session.session_id).await {
        warn!(error = %err, "failed to revoke VPN session");
    } else {
        info!(session_id = session.session_id, "revoked VPN session");
    }

    result?;
    if child_code != 0 {
        std::process::exit(child_code);
    }
    let _ = exit_ip;
    Ok(())
}

async fn print_status(config: Config) -> Result<()> {
    let status = status::read(&config.status_file).await?;
    let remaining = status.expires_at - Utc::now();
    let remaining_secs = remaining.num_seconds().max(0);
    let remaining_mins = remaining_secs / 60;
    let health = if health::check_proxy(status.proxy).await.is_ok() {
        "healthy"
    } else {
        "unhealthy"
    };

    println!("Session: {}", status.session_id);
    println!("Region: {}", status.region);
    println!("Proxy: {}", status.proxy);
    println!("Tunnel IP: {}", status.tunnel_ip.trim_end_matches("/32"));
    println!(
        "Exit IP: {}",
        status.exit_ip.unwrap_or_else(|| "unknown".to_string())
    );
    println!("Status: {health}");
    println!("Expires in: {remaining_mins}m");
    Ok(())
}

fn endpoint_host_ip(endpoint: &str) -> Option<String> {
    let (host, _) = endpoint.rsplit_once(':')?;
    host.parse::<std::net::IpAddr>().ok()?;
    Some(host.to_string())
}
