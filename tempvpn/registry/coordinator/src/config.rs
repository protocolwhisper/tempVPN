use std::{env, net::SocketAddr, path::PathBuf};

use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub coordination_bind_addr: SocketAddr,
    pub database_path: PathBuf,
    pub token_key_path: PathBuf,
    pub token_key_version: u32,
    pub server_certificate_path: PathBuf,
    pub server_private_key_path: PathBuf,
    pub client_root_ca_path: PathBuf,
    pub intermediate_certificate_path: PathBuf,
    pub intermediate_private_key_path: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bind_addr = env::var("COORDINATOR_BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
            .parse()
            .map_err(|error| Error::Config(format!("invalid COORDINATOR_BIND_ADDR: {error}")))?;
        let coordination_bind_addr = env::var("COORDINATOR_COORDINATION_BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8443".to_string())
            .parse()
            .map_err(|error| {
                Error::Config(format!(
                    "invalid COORDINATOR_COORDINATION_BIND_ADDR: {error}"
                ))
            })?;
        let database_path = PathBuf::from(
            env::var("COORDINATOR_DATABASE_PATH")
                .unwrap_or_else(|_| "/var/lib/tempvpn-coordinator/coordinator.sqlite".to_string()),
        );
        let token_key_path = PathBuf::from(
            env::var("COORDINATOR_TOKEN_KEY_FILE")
                .map_err(|_| Error::Config("COORDINATOR_TOKEN_KEY_FILE is required".into()))?,
        );
        let token_key_version = env::var("COORDINATOR_TOKEN_KEY_VERSION")
            .unwrap_or_else(|_| "1".into())
            .parse::<u32>()
            .map_err(|error| Error::Config(format!("invalid token key version: {error}")))?;
        if token_key_version == 0 {
            return Err(Error::Config("token key version must be positive".into()));
        }
        let required_path = |name: &'static str| {
            env::var(name)
                .map(PathBuf::from)
                .map_err(|_| Error::Config(format!("{name} is required")))
        };
        Ok(Self {
            bind_addr,
            coordination_bind_addr,
            database_path,
            token_key_path,
            token_key_version,
            server_certificate_path: required_path("COORDINATOR_SERVER_CERT_FILE")?,
            server_private_key_path: required_path("COORDINATOR_SERVER_KEY_FILE")?,
            client_root_ca_path: required_path("COORDINATOR_CLIENT_ROOT_CA_FILE")?,
            intermediate_certificate_path: required_path("COORDINATOR_INTERMEDIATE_CERT_FILE")?,
            intermediate_private_key_path: required_path("COORDINATOR_INTERMEDIATE_KEY_FILE")?,
        })
    }
}
