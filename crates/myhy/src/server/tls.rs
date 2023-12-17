use anyhow::Context as _;
use rustls_pemfile::private_key;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::path::Path;
use std::sync::Arc;
use std::vec::Vec;
use std::{fs, io};
use tokio_rustls::{rustls, TlsAcceptor};

pub struct TlsConfig {
    pub cert_file: String,
    pub key_file: String,
}

pub fn tls_acceptor(ssl_config: &TlsConfig) -> anyhow::Result<TlsAcceptor> {
    // Build TLS configuration.
    let tls_cfg = {
        // Load public certificate.
        let certs = load_certs(&ssl_config.cert_file)?;
        // Load private key.
        let key = load_private_key(&ssl_config.key_file)?;
        // Do not use client certificate authentication.
        let mut cfg = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;
        // Configure ALPN to accept HTTP/2, HTTP/1.1 in that order.
        cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Arc::new(cfg)
    };

    Ok(TlsAcceptor::from(tls_cfg))
}

// Load public certificate from file.
fn load_certs(filename: impl AsRef<Path>) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    // Open certificate file.
    let certfile = fs::File::open(filename).context("open certificate file")?;
    let mut reader = io::BufReader::new(certfile);
    // Load and return certificate.
    let certs = rustls_pemfile::certs(&mut reader);
    certs.map(|r| r.map_err(anyhow::Error::from)).collect()
}

// Loads first private key from file.
fn load_private_key(
    filename: impl AsRef<Path> + std::fmt::Debug,
) -> anyhow::Result<PrivateKeyDer<'static>> {
    let keyfile = fs::File::open(filename.as_ref()).context("open private key file")?;
    let mut reader = io::BufReader::new(keyfile);

    let res = private_key(&mut reader)?
        .ok_or_else(|| anyhow::anyhow!("no private key found in {:?}", filename));

    res
}
