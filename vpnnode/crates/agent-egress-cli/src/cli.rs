use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::error::{Error, Result};

#[derive(Debug, Parser)]
#[command(name = "agent-egress")]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Run(RunArgs),
    Status,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long, default_value = "us")]
    pub region: String,

    #[arg(long, default_value = "30m", value_parser = parse_duration_seconds)]
    pub duration: u64,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    pub command: Vec<String>,
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
