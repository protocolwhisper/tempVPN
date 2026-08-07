mod cli;
mod config;
mod error;
mod health;
mod helpers;
mod keygen;
mod node_client;
mod process;
mod proxy;
mod status;
mod wireguard_client;

use std::path::PathBuf;

use clap::Parser;
use tokio::fs;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    cli::{Cli, Command},
    config::Config,
    error::{Error, Result},
    node_client::{DiscoveryFilters, NodeClient},
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
        Command::Heartbeat => heartbeat(config).await,
        Command::Config(args) => generate_config(config, args).await,
        Command::Select(args) => select_node(config, args).await,
        Command::Check(args) => check_node(config, args).await,
        Command::Status => print_status(config).await,
    }
}

async fn check_node(config: Config, args: cli::CheckArgs) -> Result<()> {
    let node = NodeClient::for_base_url(args.node_url, &config);
    node.check_available().await?;
    if args.json {
        println!(
            "{}",
            serde_json::json!({ "status": "available", "node_url": node.base_url() })
        );
    } else {
        println!("available: {}", node.base_url());
    }
    Ok(())
}

async fn select_node(config: Config, args: cli::SelectArgs) -> Result<()> {
    let filters = discovery_filters(&args.selection)?;
    let node = NodeClient::select(&config, &filters, args.selection.node_url.as_deref()).await?;
    if args.json {
        let selected = node.selected_node();
        println!(
            "{}",
            serde_json::json!({
                "node_url": node.base_url(),
                "node_id": selected.map(|node| node.id.as_str()),
                "node_name": selected.map(|node| node.name.as_str()),
                "country_code": selected.and_then(|node| node.country_code.as_deref()),
                "subdivision_code": selected.and_then(|node| node.subdivision_code.as_deref()),
                "city": selected.and_then(|node| node.city.as_deref()),
                "region": selected.map(|node| node.region.as_str()),
                "expected_exit_ip": selected.map(|node| node.expected_exit_ip.as_str()),
                "selection_policy": "lowest_latency"
            })
        );
    } else {
        println!("{}", node.base_url());
    }
    Ok(())
}

async fn run(config: Config, args: cli::RunArgs) -> Result<()> {
    let (keypair, session, node_url) = get_session(
        &config,
        args.duration,
        args.session_response.as_ref(),
        args.private_key_path.as_ref(),
        &args.selection,
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
            .or_else(|| Some(session.expected_exit_ip.clone()))
            .or_else(|| helpers::endpoint_host_ip(&session.endpoint));
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
            node_url: node_url.clone(),
            proxy: proxy_addr,
            tunnel_ip: session.assigned_ip.clone(),
            exit_ip: Some(observed_exit_ip),
            interface_name: config.interface_name.clone(),
            config_path: None,
            not_after: session.not_after,
            remaining_seconds: session.remaining_seconds,
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
    if let Err(err) = NodeClient::for_base_url(node_url, &config)
        .pause_session(&session.session_id)
        .await
    {
        warn!(error = %err, "failed to pause server session");
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
    let (keypair, session, node_url) = get_session(
        &config,
        args.duration,
        args.session_response.as_ref(),
        args.private_key_path.as_ref(),
        &args.selection,
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
        let _ = NodeClient::for_base_url(node_url.clone(), &config)
            .pause_session(&session.session_id)
            .await;
        return Err(err);
    }

    if !wireguard_client::interface_is_active(&config.wg_command, &config.interface_name).await {
        let _ = wireguard_client::down_config(&config.wg_quick_command, &config_path).await;
        let _ = fs::remove_file(&config_path).await;
        let _ = NodeClient::for_base_url(node_url.clone(), &config)
            .pause_session(&session.session_id)
            .await;
        return Err(Error::TunnelInactive(config.interface_name));
    }

    // The interface is already active at this point. Treat the external IP
    // lookup as diagnostics: transient DNS/HTTP failures must not undo a
    // successful connection and make the demo appear to connect, then die.
    let observed_exit_ip = match health::visible_ip_direct().await {
        Ok(ip) => {
            if let Some(expected) = config
                .expected_exit_ip
                .as_ref()
                .or(Some(&session.expected_exit_ip))
            {
                if ip != *expected {
                    warn!(expected, observed = ip, "exit IP verification mismatch");
                }
            }
            Some(ip)
        }
        Err(err) => {
            warn!(error = %err, "exit IP verification failed; leaving tunnel connected");
            None
        }
    };

    StatusFile {
        session_id: session.session_id.clone(),
        node_url,
        proxy: config.proxy_addr,
        tunnel_ip: session.assigned_ip.clone(),
        exit_ip: observed_exit_ip.clone(),
        interface_name: config.interface_name.clone(),
        config_path: Some(config_path.clone()),
        not_after: session.not_after,
        remaining_seconds: session.remaining_seconds,
    }
    .write(&config.status_file)
    .await?;

    println!("Connected: {}", config.interface_name);
    println!("Session: {}", session.session_id);
    println!(
        "Assigned IP: {}",
        session.assigned_ip.trim_end_matches("/32")
    );
    println!(
        "Exit IP: {}",
        observed_exit_ip.unwrap_or_else(|| "verification unavailable".to_string())
    );
    println!("Config: {}", config_path.display());
    println!("Remaining: {}s", session.remaining_seconds);
    println!("Use before: {}", session.not_after);
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

    NodeClient::for_base_url(status.node_url.clone(), &config)
        .pause_session(&status.session_id)
        .await?;
    status::remove(&config.status_file).await;
    println!("Disconnected: {}", status.interface_name);
    println!("Session paused: {}", status.session_id);
    Ok(())
}

async fn heartbeat(config: Config) -> Result<()> {
    let mut status = status::read(&config.status_file).await?;
    let session = NodeClient::for_base_url(status.node_url.clone(), &config)
        .heartbeat(&status.session_id)
        .await?;
    status.remaining_seconds = session.remaining_seconds;
    status.not_after = session.not_after;
    status.write(&config.status_file).await?;
    println!("Session: {}", session.session_id);
    println!("State: {}", session.state);
    println!("Remaining: {}s", session.remaining_seconds);
    println!("Use before: {}", session.not_after);
    Ok(())
}

async fn generate_config(config: Config, args: cli::ConfigArgs) -> Result<()> {
    let (keypair, session, _node_url) = get_session(
        &config,
        args.duration,
        args.session_response.as_ref(),
        args.private_key_path.as_ref(),
        &args.selection,
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
        println!("Remaining: {}s", session.remaining_seconds);
        println!("Use before: {}", session.not_after);
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
    selection: &cli::SelectionArgs,
) -> Result<(keygen::Keypair, node_client::Session, String)> {
    if let Some(session_response) = session_response {
        let private_key_path = private_key_path.ok_or_else(|| {
            Error::InvalidConfig("--private-key-path is required with --session-response".into())
        })?;
        let private_key = fs::read_to_string(private_key_path).await?;
        let private_key = private_key.trim().to_string();
        let public_key = keygen::public_key(&config.wg_command, &private_key).await?;
        let keypair = keygen::Keypair {
            private_key,
            public_key,
        };
        let raw_session = fs::read_to_string(session_response).await?;
        if let Ok(session) = serde_json::from_str::<node_client::Session>(&raw_session) {
            enforce_selected_node(selection.node_url.as_deref(), &session.node_url)?;
            let node_url = session.node_url.clone();
            return Ok((keypair, session, node_url));
        }
        let created = serde_json::from_str::<node_client::CreatedSession>(&raw_session)?;
        enforce_selected_node(selection.node_url.as_deref(), &created.node_url)?;
        let node = NodeClient::for_base_url(created.node_url.clone(), config);
        let session = node
            .connect_session(&created.session_id, &keypair.public_key)
            .await?;
        return Ok((keypair, session, node.base_url().to_string()));
    }

    let filters = discovery_filters(selection)?;
    let node = NodeClient::select(config, &filters, selection.node_url.as_deref()).await?;
    let node_url = node.base_url().to_string();
    let keypair = keygen::generate(&config.wg_command).await?;
    info!("generated ephemeral WireGuard keypair");
    let created = node.create_session(duration).await?;
    let session = node
        .connect_session(&created.session_id, &keypair.public_key)
        .await?;
    Ok((keypair, session, node_url))
}

fn discovery_filters(selection: &cli::SelectionArgs) -> Result<DiscoveryFilters> {
    let _policy = selection.selection_policy;
    DiscoveryFilters::new(
        selection.country.as_deref(),
        selection.city.as_deref(),
        selection.region.as_deref(),
    )
}

fn enforce_selected_node(override_url: Option<&str>, paid_node_url: &str) -> Result<()> {
    if let Some(override_url) = override_url {
        if override_url.trim_end_matches('/') != paid_node_url.trim_end_matches('/') {
            return Err(Error::InvalidConfig(format!(
                "paid session belongs to {paid_node_url}, not {override_url}"
            )));
        }
    }
    Ok(())
}

async fn print_status(config: Config) -> Result<()> {
    let status = status::read(&config.status_file).await?;
    let remaining_secs = status.remaining_seconds as i64;
    let remaining_mins = remaining_secs / 60;

    let is_connect = status.config_path.is_some();
    let (is_healthy, exit_ip) = if is_connect {
        match health::visible_ip_direct().await {
            Ok(observed) => {
                let matches_expected = config
                    .expected_exit_ip
                    .as_ref()
                    .map(|expected| expected == &observed)
                    .unwrap_or(true);
                (matches_expected, Some(observed))
            }
            Err(_) => (false, None),
        }
    } else {
        (
            health::check_proxy(status.proxy).await.is_ok(),
            status.exit_ip.clone(),
        )
    };
    let health = if remaining_secs > 0 && is_healthy {
        "healthy"
    } else {
        "unhealthy"
    };

    println!("Session: {}", status.session_id);
    if is_connect {
        println!("Interface: {}", status.interface_name);
    } else {
        println!("Proxy: {}", status.proxy);
    }
    println!("Tunnel IP: {}", status.tunnel_ip.trim_end_matches("/32"));
    println!(
        "Exit IP: {}",
        exit_ip.unwrap_or_else(|| "unknown".to_string())
    );
    println!("Status: {health}");
    println!("Remaining: {remaining_mins}m");
    println!("Use before: {}", status.not_after);
    Ok(())
}

fn default_wireguard_config_path(interface_name: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/{interface_name}.conf"))
}

#[cfg(test)]
mod selection_tests {
    use super::enforce_selected_node;

    #[test]
    fn paid_session_cannot_be_migrated_to_an_override() {
        assert!(enforce_selected_node(Some("https://node-a"), "https://node-a/").is_ok());
        assert!(enforce_selected_node(Some("https://node-b"), "https://node-a").is_err());
    }
}
