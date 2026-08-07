use std::sync::Arc;

use tempvpn_session_coordinator::{
    config::Config,
    coordination_router,
    crypto::TokenCipher,
    pki::{mtls_server_config, CertificateAuthority},
    router,
    store::Store,
    AppState,
};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env()?;
    let store = Store::open(&config.database_path)?;
    let token_cipher = Arc::new(TokenCipher::from_file(
        &config.token_key_path,
        config.token_key_version,
    )?);
    let certificate_authority = Arc::new(CertificateAuthority::from_files(
        &config.intermediate_certificate_path,
        &config.intermediate_private_key_path,
    )?);
    let tls = axum_server::tls_rustls::RustlsConfig::from_config(mtls_server_config(
        &config.server_certificate_path,
        &config.server_private_key_path,
        &config.client_root_ca_path,
    )?);
    let state = AppState {
        store,
        token_cipher,
        certificate_authority: Some(certificate_authority),
    };
    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(address = %config.bind_addr, database = %config.database_path.display(), "session coordinator public API listening");
    info!(address = %config.coordination_bind_addr, "session coordinator mTLS API listening");
    let public = axum::serve(listener, router(state.clone()));
    let private = axum_server::bind(config.coordination_bind_addr)
        .acceptor(axum_server_mtls::MtlsAcceptor::new(
            axum_server::tls_rustls::RustlsAcceptor::new(tls),
        ))
        .serve(coordination_router(state).into_make_service());
    tokio::try_join!(public, private)?;
    Ok(())
}
