use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::error::{Error, Result};

#[derive(Debug, Parser)]
#[command(name = "vpn-client")]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run(RunArgs),
    Connect(ConnectArgs),
    Disconnect,
    Heartbeat,
    Config(ConfigArgs),
    Select(SelectArgs),
    Check(CheckArgs),
    Status,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[arg(long)]
    pub node_url: String,

    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SelectArgs {
    #[command(flatten)]
    pub selection: SelectionArgs,

    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub selection: SelectionArgs,

    #[arg(long, default_value = "30m", value_parser = parse_duration_seconds)]
    pub duration: u64,

    #[arg(long)]
    pub session_response: Option<PathBuf>,

    #[arg(long)]
    pub private_key_path: Option<PathBuf>,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ConnectArgs {
    #[command(flatten)]
    pub selection: SelectionArgs,

    #[arg(long, default_value = "30m", value_parser = parse_duration_seconds)]
    pub duration: u64,

    #[arg(long)]
    pub config_path: Option<PathBuf>,

    #[arg(long)]
    pub session_response: Option<PathBuf>,

    #[arg(long)]
    pub private_key_path: Option<PathBuf>,

    #[arg(long, default_value = "0.0.0.0/0")]
    pub allowed_ips: String,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(flatten)]
    pub selection: SelectionArgs,

    #[arg(long, default_value = "30m", value_parser = parse_duration_seconds)]
    pub duration: u64,

    #[arg(long)]
    pub output: Option<PathBuf>,

    #[arg(long)]
    pub session_response: Option<PathBuf>,

    #[arg(long)]
    pub private_key_path: Option<PathBuf>,

    #[arg(long, default_value = "0.0.0.0/0")]
    pub allowed_ips: String,
}

#[derive(Debug, Clone, Default, Args)]
pub struct SelectionArgs {
    /// Registry control-plane origin for discovery, payment, and lifecycle calls.
    #[arg(long)]
    pub registry_url: Option<String>,

    /// Select this exact live catalog node by ID.
    #[arg(long, conflicts_with = "node_url")]
    pub node_id: Option<String>,

    /// Only consider nodes advertising this ISO 3166-1 alpha-2 country code.
    #[arg(long)]
    pub country: Option<String>,

    /// Only consider nodes advertising this city.
    #[arg(long)]
    pub city: Option<String>,

    /// Only consider nodes advertising this region.
    #[arg(long)]
    pub region: Option<String>,

    /// Select the live catalog node advertising this diagnostic API URL.
    #[arg(long, conflicts_with = "node_id")]
    pub node_url: Option<String>,

    /// Policy used to rank eligible nodes from this device.
    #[arg(long, value_enum, default_value_t)]
    pub selection_policy: SelectionPolicy,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum SelectionPolicy {
    #[default]
    LowestLatency,
}

fn parse_duration_seconds(raw: &str) -> Result<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(Error::InvalidDuration(
            "duration cannot be empty".to_string(),
        ));
    }

    let (number, multiplier) = match raw.chars().last().unwrap() {
        's' => (&raw[..raw.len() - 1], 1),
        'm' => (&raw[..raw.len() - 1], 60),
        'h' => (&raw[..raw.len() - 1], 60 * 60),
        _ => (raw, 1),
    };
    let value = number.parse::<u64>()?;
    Ok(value * multiplier)
}
