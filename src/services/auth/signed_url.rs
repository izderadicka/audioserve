use std::sync::Arc;

use data_encoding::BASE64URL_NOPAD;
use ring::hmac;

use anyhow::{anyhow, bail, Context};

use crate::services::auth::now;

/// Signs and verifies URLs of the form:
///
///   /some/path?exp=<unix_ms>&sig=<base64url_hmac>
///
/// Signature input is:
///
///   [path_len: u16 big endian] || [path bytes] || [exp: u64 big endian]
///
/// Using the length prefix avoids ambiguity in the signed payload.
struct SignedUrlServiceInner {
    key: hmac::Key,
    token_validity_seconds: u32,
}

impl SignedUrlServiceInner {
    pub fn new(secret: Vec<u8>, token_validity_seconds: u32) -> Self {
        assert!(!secret.is_empty(), "secret must not be empty");

        Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, &secret),
            token_validity_seconds,
        }
    }

    fn prepare_data(path: &str, exp_ms: u64) -> Vec<u8> {
        assert!(path.len() <= u16::MAX as usize, "path too long");

        let mut data = Vec::with_capacity(2 + path.len() + 8);
        let path_len = (path.len() as u16).to_be_bytes();
        data.extend_from_slice(&path_len);
        data.extend_from_slice(path.as_bytes());
        data.extend_from_slice(&exp_ms.to_be_bytes());
        data
    }

    fn sign_path_and_exp(&self, path: &str, exp_ms: u64) -> String {
        let data = Self::prepare_data(path, exp_ms);
        let sig = hmac::sign(&self.key, &data);
        BASE64URL_NOPAD.encode(sig.as_ref())
    }

    /// Creates signed query parameters for a given path.
    ///
    /// Returns:
    ///   (exp_ms, sig)
    pub fn create_path_signature(&self, path: &str) -> (u64, String) {
        let exp_ms = now() + u64::from(self.token_validity_seconds) * 1000;
        let sig = self.sign_path_and_exp(path, exp_ms);
        (exp_ms, sig)
    }

    /// Verifies that `sig` is valid for the exact `path` and `exp_ms`.
    pub fn verify_path_signature(&self, path: &str, exp_ms: u64, sig: &str) -> anyhow::Result<()> {
        if exp_ms < now() {
            bail!("Token expired");
        }

        let sig_bytes = BASE64URL_NOPAD
            .decode(sig.as_bytes())
            .context("Invalid signature encoding")?;

        // HMAC-SHA256 output length is 32 bytes.
        if sig_bytes.len() != 32 {
            bail!("Invalid signature length");
        }

        let data = Self::prepare_data(path, exp_ms);

        hmac::verify(&self.key, &data, &sig_bytes)
            .map_err(|e| anyhow!("Invalid signature: {e}"))?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct SignedUrlService {
    inner: Arc<SignedUrlServiceInner>,
}

impl SignedUrlService {
    pub fn new(secret: Vec<u8>, token_validity_seconds: u32) -> Self {
        Self {
            inner: Arc::new(SignedUrlServiceInner::new(secret, token_validity_seconds)),
        }
    }

    /// Returns `(exp_ms, sig)`.
    pub fn create_signature(&self, path: &str) -> (u64, String) {
        self.inner.create_path_signature(path)
    }

    pub fn verify_signature(&self, path: &str, exp_ms: u64, sig: &str) -> anyhow::Result<()> {
        self.inner.verify_path_signature(path, exp_ms, sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signed_url_service_ok() {
        let service = SignedUrlService::new(vec![7u8; 32], 10);
        let path = "/icon/test";

        let (exp, sig) = service.create_signature(path);
        service.verify_signature(path, exp, &sig).unwrap();
    }

    #[test]
    fn test_signed_url_service_wrong_path() {
        let service = SignedUrlService::new(vec![7u8; 32], 10);
        let (exp, sig) = service.create_signature("/icon/test");

        let err = service.verify_signature("/icon/other", exp, &sig);
        assert!(err.is_err());
    }

    #[test]
    fn test_signed_url_service_bad_sig() {
        let service = SignedUrlService::new(vec![7u8; 32], 10);
        let (exp, mut sig) = service.create_signature("/icon/test");

        sig.push('x');

        let err = service.verify_signature("/icon/test", exp, &sig);
        assert!(err.is_err());
    }

    #[test]
    fn test_signed_url_service_expired() {
        let service = SignedUrlService::new(vec![7u8; 32], 0);
        let path = "/icon/test";

        let (exp, sig) = service.create_signature(path);

        // Depending on how `now()` is implemented, immediate expiry may already be expired.
        let err = service.verify_signature(path, exp.saturating_sub(1), &sig);
        assert!(err.is_err());
    }
}
