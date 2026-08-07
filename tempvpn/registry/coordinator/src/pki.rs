use std::{
    fs,
    io::{BufReader, Cursor},
    path::Path,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use rcgen::{
    CertificateParams, CertificateSigningRequestParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose, SanType,
};
use rustls::{
    crypto::ring::default_provider,
    pki_types::{CertificateDer, PrivateKeyDer},
    server::WebPkiClientVerifier,
    RootCertStore, ServerConfig,
};
use time::{Duration, OffsetDateTime};
use x509_parser::{extensions::GeneralName, parse_x509_certificate};

use crate::{types::CoordinationIdentity, Error, Result};

const CERTIFICATE_VALID_HOURS: i64 = 24;
const CLOCK_SKEW_MINUTES: i64 = 5;

pub struct CertificateAuthority {
    issuer: Issuer<'static, KeyPair>,
    intermediate_certificate_pem: String,
}

impl std::fmt::Debug for CertificateAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CertificateAuthority")
            .finish_non_exhaustive()
    }
}

impl CertificateAuthority {
    pub fn from_files(certificate_path: &Path, private_key_path: &Path) -> Result<Self> {
        let certificate_pem = fs::read_to_string(certificate_path).map_err(|error| {
            Error::Certificate(format!(
                "failed reading intermediate certificate {}: {error}",
                certificate_path.display()
            ))
        })?;
        let private_key_pem = fs::read_to_string(private_key_path).map_err(|error| {
            Error::Certificate(format!(
                "failed reading intermediate private key {}: {error}",
                private_key_path.display()
            ))
        })?;
        Self::from_pem(&certificate_pem, &private_key_pem)
    }

    pub fn from_pem(certificate_pem: &str, private_key_pem: &str) -> Result<Self> {
        let key_pair = KeyPair::from_pem(private_key_pem)
            .map_err(|error| Error::Certificate(format!("invalid intermediate key: {error}")))?;
        let issuer = Issuer::from_ca_cert_pem(certificate_pem, key_pair).map_err(|error| {
            Error::Certificate(format!("invalid intermediate certificate: {error}"))
        })?;
        Ok(Self {
            issuer,
            intermediate_certificate_pem: certificate_pem.trim().to_string(),
        })
    }

    pub fn issue(
        &self,
        certificate_signing_request_pem: &str,
        identity: &CoordinationIdentity,
    ) -> Result<crate::types::CertificateResponse> {
        let mut request = CertificateSigningRequestParams::from_pem(
            certificate_signing_request_pem,
        )
        .map_err(|error| Error::Certificate(format!("invalid certificate request: {error}")))?;
        let now = OffsetDateTime::now_utc();
        let expires = now + Duration::hours(CERTIFICATE_VALID_HOURS);
        let san = identity.san_uri().try_into().map_err(|_| {
            Error::Certificate("identity cannot be represented as a SAN URI".into())
        })?;
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, identity.san_uri());
        let mut params = CertificateParams::default();
        params.not_before = now - Duration::minutes(CLOCK_SKEW_MINUTES);
        params.not_after = expires;
        params.subject_alt_names = vec![SanType::URI(san)];
        params.distinguished_name = distinguished_name;
        params.is_ca = IsCa::NoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        request.params = params;
        let certificate = request
            .signed_by(&self.issuer)
            .map_err(|error| Error::Certificate(format!("could not sign certificate: {error}")))?;
        let expires_at = DateTime::<Utc>::from_timestamp(expires.unix_timestamp(), 0)
            .ok_or_else(|| Error::Certificate("certificate expiration is out of range".into()))?;
        Ok(crate::types::CertificateResponse {
            certificate_chain_pem: format!(
                "{}\n{}\n",
                certificate.pem(),
                self.intermediate_certificate_pem
            ),
            expires_at,
        })
    }
}

pub fn identity_from_certificate(certificate: &CertificateDer<'_>) -> Result<CoordinationIdentity> {
    let (_, certificate) = parse_x509_certificate(certificate.as_ref())
        .map_err(|_| Error::Certificate("could not parse client certificate".into()))?;
    let san = certificate
        .subject_alternative_name()
        .map_err(|_| Error::Certificate("invalid client certificate SAN".into()))?
        .ok_or(Error::Unauthorized)?;
    let identities = san
        .value
        .general_names
        .iter()
        .filter_map(|name| match name {
            GeneralName::URI(uri) => CoordinationIdentity::from_san_uri(uri),
            _ => None,
        })
        .collect::<Vec<_>>();
    match identities.as_slice() {
        [identity] => Ok(identity.clone()),
        _ => Err(Error::Unauthorized),
    }
}

impl CoordinationIdentity {
    fn from_san_uri(value: &str) -> Option<Self> {
        if value == "spiffe://tempvpn/operator" {
            return Some(Self::Operator);
        }
        let suffix = value.strip_prefix("spiffe://tempvpn/node/")?;
        let mut segments = suffix.split('/');
        let logical_node = segments.next()?;
        let generation_id = segments.next()?;
        if segments.next().is_some()
            || !valid_identifier(logical_node)
            || !valid_identifier(generation_id)
        {
            return None;
        }
        Some(Self::Node {
            logical_node: logical_node.to_string(),
            generation_id: generation_id.to_string(),
        })
    }
}

pub fn mtls_server_config(
    server_certificate_path: &Path,
    server_private_key_path: &Path,
    client_root_ca_path: &Path,
) -> Result<Arc<ServerConfig>> {
    let server_certificates = read_certificates(server_certificate_path)?;
    let server_private_key = read_private_key(server_private_key_path)?;
    let client_roots = read_certificates(client_root_ca_path)?;
    let mut root_store = RootCertStore::empty();
    for certificate in client_roots {
        root_store
            .add(certificate)
            .map_err(|error| Error::Certificate(format!("invalid client root CA: {error}")))?;
    }
    let provider = Arc::new(default_provider());
    let verifier = WebPkiClientVerifier::builder_with_provider(root_store.into(), provider.clone())
        .allow_unauthenticated()
        .build()
        .map_err(|error| Error::Certificate(format!("invalid client verifier: {error}")))?;
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| Error::Certificate(format!("invalid TLS protocol config: {error}")))?
        .with_client_cert_verifier(verifier)
        .with_single_cert(server_certificates, server_private_key)
        .map_err(|error| Error::Certificate(format!("invalid server identity: {error}")))?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

fn read_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let bytes = fs::read(path).map_err(|error| {
        Error::Certificate(format!(
            "failed reading certificate {}: {error}",
            path.display()
        ))
    })?;
    let certificates = rustls_pemfile::certs(&mut BufReader::new(Cursor::new(bytes)))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| Error::Certificate(format!("invalid certificate PEM: {error}")))?;
    if certificates.is_empty() {
        return Err(Error::Certificate(format!(
            "certificate file {} is empty",
            path.display()
        )));
    }
    Ok(certificates)
}

fn read_private_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let bytes = fs::read(path).map_err(|error| {
        Error::Certificate(format!(
            "failed reading private key {}: {error}",
            path.display()
        ))
    })?;
    rustls_pemfile::private_key(&mut BufReader::new(Cursor::new(bytes)))
        .map_err(|error| Error::Certificate(format!("invalid private key PEM: {error}")))?
        .ok_or_else(|| Error::Certificate(format!("private key file {} is empty", path.display())))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, CertifiedKey};

    fn authority() -> (CertificateAuthority, rcgen::Certificate) {
        let root_key = KeyPair::generate().unwrap();
        let mut root_params = CertificateParams::default();
        root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        root_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::CrlSign,
        ];
        let root = root_params.self_signed(&root_key).unwrap();

        let intermediate_key = KeyPair::generate().unwrap();
        let mut intermediate_params = CertificateParams::default();
        intermediate_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        intermediate_params.key_usages = root_params.key_usages.clone();
        let intermediate = intermediate_params
            .signed_by(
                &intermediate_key,
                &Issuer::from_params(&root_params, &root_key),
            )
            .unwrap();
        let authority =
            CertificateAuthority::from_pem(&intermediate.pem(), &intermediate_key.serialize_pem())
                .unwrap();
        (authority, root)
    }

    #[test]
    fn issues_generation_identity_from_verified_csr_and_ignores_requested_sans() {
        let (authority, _) = authority();
        let subject_key = KeyPair::generate().unwrap();
        let requested = CertificateParams::new(vec!["attacker.test".to_string()])
            .unwrap()
            .serialize_request(&subject_key)
            .unwrap();
        let identity = CoordinationIdentity::Node {
            logical_node: "node-a".into(),
            generation_id: "green".into(),
        };
        let issued = authority
            .issue(&requested.pem().unwrap(), &identity)
            .unwrap();
        let certificates = rustls_pemfile::certs(&mut BufReader::new(Cursor::new(
            issued.certificate_chain_pem.as_bytes(),
        )))
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
        assert_eq!(
            identity_from_certificate(&certificates[0]).unwrap(),
            identity
        );
    }

    #[test]
    fn rejects_certificates_without_exactly_one_tempvpn_identity() {
        let CertifiedKey { cert, .. } =
            rcgen::generate_simple_self_signed(vec!["unrelated.test".into()]).unwrap();
        assert!(matches!(
            identity_from_certificate(cert.der()),
            Err(Error::Unauthorized)
        ));
    }
}
