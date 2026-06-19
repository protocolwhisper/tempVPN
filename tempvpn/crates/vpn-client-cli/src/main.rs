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

use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;
use tokio::fs;
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
                .unwrap_or_else(|_| "vpn_client=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    let config = Config::load(cli.config).await?;

    match cli.command {
        Command::Run(args) => run(config, args).await,
        Command::Connect(args) => connect(config, args).await,
        Command::Disconnect => disconnect(config).await,
        Command::Config(args) => generate_config(config, args).await,
        Command::Status => print_status(config).await,
    }
}

async fn run(config: Config, args: cli::RunArgs) -> Result<()> {
    let (keypair, session) = get_session(
        &config,
        args.duration,
        args.session_response.as_ref(),
        args.private_key_path.as_ref(),
    )
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
            proxy: proxy_addr,
            tunnel_ip: session.assigned_ip.clone(),
            exit_ip: Some(observed_exit_ip),
            interface_name: config.interface_name.clone(),
            config_path: None,
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
    info!(
        session_id = session.session_id,
        "local VPN resources stopped; paid session will expire automatically"
    );

    result?;
    if child_code != 0 {
        std::process::exit(child_code);
    }
    let _ = exit_ip;
    Ok(())
}

async fn connect(config: Config, args: cli::ConnectArgs) -> Result<()> {
    let (keypair, session) = get_session(
        &config,
        args.duration,
        args.session_response.as_ref(),
        args.private_key_path.as_ref(),
    )
    .await?;
    info!(
        session_id = session.session_id,
        assigned_ip = session.assigned_ip,
        endpoint = session.endpoint,
        "created VPN session"
    );

    let config_path = args
        .config_path
        .unwrap_or_else(|| default_wireguard_config_path(&config.interface_name));
    wireguard_client::write_config_private(&config_path, &keypair, &session, &args.allowed_ips)
        .await?;

    if let Err(err) = wireguard_client::up_config(&config.wg_quick_command, &config_path).await {
        let _ = fs::remove_file(&config_path).await;
        return Err(err);
    }

    if !wireguard_client::interface_is_active(&config.wg_command, &config.interface_name).await {
        let _ = wireguard_client::down_config(&config.wg_quick_command, &config_path).await;
        let _ = fs::remove_file(&config_path).await;
        return Err(Error::TunnelInactive(config.interface_name));
    }

    StatusFile {
        session_id: session.session_id.clone(),
        proxy: config.proxy_addr,
        tunnel_ip: session.assigned_ip.clone(),
        exit_ip: None,
        interface_name: config.interface_name.clone(),
        config_path: Some(config_path.clone()),
        expires_at: session.expires_at,
    }
    .write(&config.status_file)
    .await?;

    println!("Connected: {}", config.interface_name);
    println!("Session: {}", session.session_id);
    println!(
        "Assigned IP: {}",
        session.assigned_ip.trim_end_matches("/32")
    );
    println!("Config: {}", config_path.display());
    println!("Expires at: {}", session.expires_at);
    Ok(())
}

async fn disconnect(config: Config) -> Result<()> {
    let status = status::read(&config.status_file).await?;

    if let Some(config_path) = &status.config_path {
        wireguard_client::down_config(&config.wg_quick_command, config_path).await?;
        let _ = fs::remove_file(config_path).await;
    } else {
        let path = default_wireguard_config_path(&status.interface_name);
        wireguard_client::down_config(&config.wg_quick_command, &path).await?;
    }

    status::remove(&config.status_file).await;
    println!("Disconnected: {}", status.interface_name);
    println!("Session expires automatically: {}", status.session_id);
    Ok(())
}

async fn generate_config(config: Config, args: cli::ConfigArgs) -> Result<()> {
    let (keypair, session) = get_session(
        &config,
        args.duration,
        args.session_response.as_ref(),
        args.private_key_path.as_ref(),
    )
    .await?;
    let wg_config = wireguard_client::render_config(&keypair, &session, &args.allowed_ips);

    if let Some(path) = args.output {
        fs::write(&path, wg_config).await?;
        println!("Wrote WireGuard config: {}", path.display());
        println!("Session: {}", session.session_id);
        println!(
            "Assigned IP: {}",
            session.assigned_ip.trim_end_matches("/32")
        );
        println!("Expires at: {}", session.expires_at);
    } else {
        print!("{wg_config}");
    }

    Ok(())
}

async fn get_session(
    config: &Config,
    duration: u64,
    session_response: Option<&PathBuf>,
    private_key_path: Option<&PathBuf>,
) -> Result<(keygen::Keypair, node_client::Session)> {
    if let Some(session_response) = session_response {
        let private_key_path = private_key_path.ok_or_else(|| {
            Error::InvalidConfig("--private-key-path is required with --session-response".into())
        })?;
        let private_key = fs::read_to_string(private_key_path).await?;
        let session = fs::read_to_string(session_response).await?;
        return Ok((
            keygen::Keypair {
                private_key: private_key.trim().to_string(),
                public_key: String::new(),
            },
            serde_json::from_str(&session)?,
        ));
    }

    let node = NodeClient::new(config);
    let keypair = keygen::generate(&config.wg_command).await?;
    info!("generated ephemeral WireGuard keypair");
    let session = node.create_session(&keypair.public_key, duration).await?;
    Ok((keypair, session))
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

fn default_wireguard_config_path(interface_name: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/{interface_name}.conf"))
}
