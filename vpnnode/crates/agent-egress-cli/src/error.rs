use std::{
    net::{AddrParseError, SocketAddr},
    num::ParseIntError,
    path::PathBuf,
};

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

    #[error("invalid duration: {0}")]
    InvalidDuration(String),

    #[error("invalid config: {0}")]
    InvalidConfig(String),

    #[error("MVP only supports --region us")]
    UnsupportedRegion,

    #[error("missing command to run")]
    MissingCommand,

    #[error("exit IP mismatch: expected {expected}, observed {observed}")]
    ExitIpMismatch { expected: String, observed: String },

    #[error("{program} failed: {stderr}")]
    CommandFailed { program: String, stderr: String },

    #[error("{operation} failed with {status}: {body}")]
    HttpStatus {
        operation: &'static str,
        status: reqwest::StatusCode,
        body: String,
    },

    #[error("SOCKS5 proxy must bind to loopback, got {0}")]
    ProxyMustBeLoopback(SocketAddr),

    #[error("unsupported SOCKS version {0}")]
    UnsupportedSocksVersion(u8),

    #[error("SOCKS client did not offer no-auth method")]
    SocksNoAuthUnavailable,

    #[error("only SOCKS5 CONNECT is supported")]
    SocksConnectOnly,

    #[error("unsupported SOCKS address type {0}")]
    UnsupportedSocksAddressType(u8),

    #[error("domain target is not valid UTF-8")]
    InvalidSocksDomain(#[source] std::string::FromUtf8Error),

    #[error("WireGuard interface {0} is not active")]
    TunnelInactive(String),

    #[error("exit IP check failed with {0}")]
    ExitIpCheckStatus(reqwest::StatusCode),

    #[error("wg pubkey stdin unavailable")]
    MissingPubkeyStdin,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    AddrParse(#[from] AddrParseError),

    #[error(transparent)]
    ParseInt(#[from] ParseIntError),

    #[error(transparent)]
    ParseBool(#[from] std::str::ParseBoolError),

    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
}
