use std::sync::Arc;

use data_encoding::BASE64URL_NOPAD;
use ring::{
    hmac,
    rand::{SecureRandom as _, SystemRandom},
};

use crate::services::auth::now;

struct SignedUrlServiceInner {
    secret: Vec<u8>,
    token_validity_seconds: u32,
}

impl SignedUrlServiceInner {
    pub fn new(secret: Vec<u8>, token_validity_seconds: u32) -> Self {
        SignedUrlServiceInner {
            secret,
            token_validity_seconds,
        }
    }

    fn prepare_data(&self, nonce: &[u8; 32], path: &str, validity: [u8; 8]) -> Vec<u8> {
        assert!(path.len() <= u16::MAX as usize);
        let mut data = Vec::with_capacity(32 + 2 + path.len() + 8);
        data.extend_from_slice(nonce);
        let path_len = (path.len() as u16).to_be_bytes();
        data.extend_from_slice(&path_len);
        data.extend_from_slice(path.as_bytes());
        data.extend_from_slice(&validity);
        data
    }

    pub fn create_token(&self, path: String) -> String {
        let mut nonce = [0u8; 32];
        let rng = SystemRandom::new();
        rng.fill(&mut nonce).expect("Cannot generate random number");
        let validity: u64 = now() + u64::from(self.token_validity_seconds) * 1000;
        let validity: [u8; 8] = validity.to_be_bytes();
        let mut token = self.prepare_data(&nonce, &path, validity);
        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.secret);
        let sig = hmac::sign(&key, &token);
        token.extend_from_slice(sig.as_ref());
        BASE64URL_NOPAD.encode(&token)
    }

    pub fn verify_token(&self, token: &str) -> anyhow::Result<String> {
        let token = BASE64URL_NOPAD.decode(token.as_bytes())?;

        // 32 nonce + 2 path len + 8 validity + 32 hmac = 74 minimum
        const MIN_LEN: usize = 32 + 2 + 8 + 32;
        if token.len() < MIN_LEN {
            anyhow::bail!("Token too short");
        }

        let data_end = token.len() - 32;
        let (data, sig) = token.split_at(data_end);

        let key = hmac::Key::new(hmac::HMAC_SHA256, &self.secret);
        hmac::verify(&key, data, sig).map_err(|_| anyhow::anyhow!("Invalid signature"))?;

        let string_len = u16::from_be_bytes([data[32], data[33]]) as usize;
        let path_start = 34;
        let path_end = path_start + string_len;
        let validity_end = path_end + 8;

        if validity_end != data.len() {
            anyhow::bail!("Malformed token");
        }

        let path = std::str::from_utf8(&data[path_start..path_end])?;
        let validity = u64::from_be_bytes(data[path_end..validity_end].try_into()?);

        if validity < now() {
            anyhow::bail!("Token expired");
        }

        Ok(path.to_owned())
    }
}

#[derive(Clone)]
pub struct SignedUrlService {
    inner: Arc<SignedUrlServiceInner>,
}

impl SignedUrlService {
    pub fn new(secret: Vec<u8>, token_validity_seconds: u32) -> Self {
        SignedUrlService {
            inner: Arc::new(SignedUrlServiceInner::new(secret, token_validity_seconds)),
        }
    }

    pub fn create_token(&self, path: String) -> String {
        self.inner.create_token(path)
    }

    pub fn verify_token(&self, token: &str) -> anyhow::Result<String> {
        self.inner.verify_token(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signed_url_service() {
        let service = SignedUrlService::new(vec![0u8; 32], 10);
        let token = service.create_token("icon/test".to_string());
        assert_eq!(service.verify_token(&token).unwrap(), "icon/test");
    }
}
