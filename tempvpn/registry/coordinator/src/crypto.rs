use std::{fs, path::Path};

use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    digest,
    rand::{SecureRandom, SystemRandom},
};

use crate::{Error, Result};

const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct TokenCipher {
    key: std::sync::Arc<LessSafeKey>,
    version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncryptedToken {
    pub key_version: u32,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl TokenCipher {
    pub fn from_file(path: &Path, version: u32) -> Result<Self> {
        let raw = fs::read(path).map_err(|error| {
            Error::Crypto(format!(
                "failed reading token key {}: {error}",
                path.display()
            ))
        })?;
        Self::new(&parse_key(&raw)?, version)
    }

    pub fn new(key: &[u8], version: u32) -> Result<Self> {
        if version == 0 {
            return Err(Error::Crypto("token key version must be positive".into()));
        }
        let key = UnboundKey::new(&aead::CHACHA20_POLY1305, key)
            .map_err(|_| Error::Crypto("token key must contain exactly 32 bytes".into()))?;
        Ok(Self {
            key: std::sync::Arc::new(LessSafeKey::new(key)),
            version,
        })
    }

    pub fn encrypt(&self, token: &str) -> Result<EncryptedToken> {
        let mut nonce = [0_u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error::Crypto("could not generate token nonce".into()))?;
        let mut ciphertext = token.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut ciphertext,
            )
            .map_err(|_| Error::Crypto("could not encrypt session token".into()))?;
        Ok(EncryptedToken {
            key_version: self.version,
            nonce: nonce.to_vec(),
            ciphertext,
        })
    }

    pub fn decrypt(&self, encrypted: &EncryptedToken) -> Result<String> {
        if encrypted.key_version != self.version {
            return Err(Error::Crypto(format!(
                "token key version {} is unavailable",
                encrypted.key_version
            )));
        }
        let nonce: [u8; NONCE_LEN] = encrypted
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| Error::Crypto("invalid token nonce".into()))?;
        let mut ciphertext = encrypted.ciphertext.clone();
        let plaintext = self
            .key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut ciphertext,
            )
            .map_err(|_| Error::Crypto("session token authentication failed".into()))?;
        String::from_utf8(plaintext.to_vec())
            .map_err(|_| Error::Crypto("decrypted session token is not UTF-8".into()))
    }

    pub fn version(&self) -> u32 {
        self.version
    }
}

pub fn token_lookup_hash(token: &str) -> [u8; 32] {
    let value = digest::digest(&digest::SHA256, token.as_bytes());
    value.as_ref().try_into().expect("SHA-256 is 32 bytes")
}

fn parse_key(raw: &[u8]) -> Result<Vec<u8>> {
    let trimmed = raw
        .strip_suffix(b"\n")
        .unwrap_or(raw)
        .strip_suffix(b"\r")
        .unwrap_or(raw);
    if trimmed.len() == 32 {
        return Ok(trimmed.to_vec());
    }
    if trimmed.len() == 64 && trimmed.iter().all(u8::is_ascii_hexdigit) {
        let mut decoded = Vec::with_capacity(32);
        for pair in trimmed.chunks_exact(2) {
            let pair = std::str::from_utf8(pair)
                .map_err(|_| Error::Crypto("token key contains invalid hex".into()))?;
            decoded.push(
                u8::from_str_radix(pair, 16)
                    .map_err(|_| Error::Crypto("token key contains invalid hex".into()))?,
            );
        }
        return Ok(decoded);
    }
    Err(Error::Crypto(
        "token key file must contain 32 raw bytes or 64 hexadecimal characters".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_and_authenticates_tokens() {
        let cipher = TokenCipher::new(&[7_u8; 32], 3).unwrap();
        let encrypted = cipher.encrypt("sess_secret").unwrap();
        assert_ne!(encrypted.ciphertext, b"sess_secret");
        assert_eq!(cipher.decrypt(&encrypted).unwrap(), "sess_secret");

        let mut tampered = encrypted;
        tampered.ciphertext[0] ^= 1;
        assert!(cipher.decrypt(&tampered).is_err());
    }

    #[test]
    fn lookup_hash_is_stable_without_containing_token() {
        let hash = token_lookup_hash("sess_secret");
        assert_eq!(hash, token_lookup_hash("sess_secret"));
        assert_ne!(hash.as_slice(), b"sess_secret");
    }
}
