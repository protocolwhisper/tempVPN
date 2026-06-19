use std::{net::AddrParseError, num::ParseIntError, path::PathBuf};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("reading config {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("parsing config {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("{0} is required")]
    MissingConfig(&'static str),

    #[error("invalid config: {0}")]
    InvalidConfig(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("no free tunnel IPs available")]
    NoFreeTunnelIps,

    #[error("{program} failed: {stderr}")]
    CommandFailed { program: String, stderr: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    AddrParse(#[from] AddrParseError),

    #[error(transparent)]
    ParseInt(#[from] ParseIntError),

    #[error(transparent)]
    ParseBool(#[from] std::str::ParseBoolError),
}
