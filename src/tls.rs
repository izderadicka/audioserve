use anyhow::Context as _;
use core::task::{Context, Poll};
use futures_util::ready;
use hyper::server::accept::Accept;
use hyper::server::conn::{AddrIncoming, AddrStream};
use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::vec::Vec;
use std::{fs, io, sync};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::rustls::ServerConfig;

use crate::config::SslConfig;

pub fn tls_acceptor(addr: SocketAddr, ssl_config: &SslConfig) -> anyhow::Result<TlsAcceptor> {
    // Build TLS configuration.
    let tls_cfg = {
        // Load public certificate.
        let certs = load_certs(&ssl_config.cert_file)?;
        // Load private key.
        let key = load_private_key(&ssl_config.key_file)?;
        // Do not use client certificate authentication.
        let mut cfg = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;
        // Configure ALPN to accept HTTP/2, HTTP/1.1 in that order.
        cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        sync::Arc::new(cfg)
    };

    // Create a TCP listener via tokio.
    let incoming = AddrIncoming::bind(&addr)?;
    Ok(TlsAcceptor::new(tls_cfg, incoming))
}

enum State {
    Handshaking(tokio_rustls::Accept<AddrStream>),
    Streaming(tokio_rustls::server::TlsStream<AddrStream>),
}

// tokio_rustls::server::TlsStream doesn't expose constructor methods,
// so we have to TlsAcceptor::accept and handshake to have access to it
// TlsStream implements AsyncRead/AsyncWrite handshaking tokio_rustls::Accept first
pub struct TlsStream {
    state: State,
    remote_addr: SocketAddr,
}

impl TlsStream {
    fn new(stream: AddrStream, config: Arc<ServerConfig>) -> TlsStream {
        let remote_addr = stream.remote_addr();
        let accept = tokio_rustls::TlsAcceptor::from(config).accept(stream);
        TlsStream {
            state: State::Handshaking(accept),
            remote_addr,
        }
    }

    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

impl AsyncRead for TlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let pin = self.get_mut();
        match pin.state {
            State::Handshaking(ref mut accept) => match ready!(Pin::new(accept).poll(cx)) {
                Ok(mut stream) => {
                    let result = Pin::new(&mut stream).poll_read(cx, buf);
                    pin.state = State::Streaming(stream);
                    result
                }
                Err(err) => Poll::Ready(Err(err)),
            },
            State::Streaming(ref mut stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let pin = self.get_mut();
        match pin.state {
            State::Handshaking(ref mut accept) => match ready!(Pin::new(accept).poll(cx)) {
                Ok(mut stream) => {
                    let result = Pin::new(&mut stream).poll_write(cx, buf);
                    pin.state = State::Streaming(stream);
                    result
                }
                Err(err) => Poll::Ready(Err(err)),
            },
            State::Streaming(ref mut stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.state {
            State::Handshaking(_) => Poll::Ready(Ok(())),
            State::Streaming(ref mut stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.state {
            State::Handshaking(_) => Poll::Ready(Ok(())),
            State::Streaming(ref mut stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

pub struct TlsAcceptor {
    config: Arc<ServerConfig>,
    incoming: AddrIncoming,
}

impl TlsAcceptor {
    pub fn new(config: Arc<ServerConfig>, incoming: AddrIncoming) -> TlsAcceptor {
        TlsAcceptor { config, incoming }
    }
}

impl Accept for TlsAcceptor {
    type Conn = TlsStream;
    type Error = io::Error;

    fn poll_accept(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        let pin = self.get_mut();
        match ready!(Pin::new(&mut pin.incoming).poll_accept(cx)) {
            Some(Ok(sock)) => Poll::Ready(Some(Ok(TlsStream::new(sock, pin.config.clone())))),
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            None => Poll::Ready(None),
        }
    }
}

// Load public certificate from file.
fn load_certs(filename: impl AsRef<Path>) -> anyhow::Result<Vec<rustls::Certificate>> {
    // Open certificate file.
    let certfile = fs::File::open(filename).context("open certificate file")?;
    let mut reader = io::BufReader::new(certfile);

    // Load and return certificate.
    let certs = rustls_pemfile::certs(&mut reader).context("read certificates")?;
    assert!(
        !certs.is_empty(),
        "there must be at least 1 certificate in file"
    );
    Ok(certs.into_iter().map(rustls::Certificate).collect())
}

// Load private key from file.
fn load_private_key(filename: impl AsRef<Path>) -> anyhow::Result<rustls::PrivateKey> {
    // Open keyfile.
    let keyfile = fs::File::open(filename).context("open private key file")?;
    let mut reader = io::BufReader::new(keyfile);

    // Load and return a single private key.
    match rustls_pemfile::read_one(&mut reader).context("read private keys")? {
        Some(r) => match r {
            rustls_pemfile::Item::X509Certificate(_) => {
                anyhow::bail!("looks like file contains certificates")
            }
            rustls_pemfile::Item::RSAKey(data) => Ok(rustls::PrivateKey(data)),
            rustls_pemfile::Item::PKCS8Key(data) => Ok(rustls::PrivateKey(data)),
            rustls_pemfile::Item::ECKey(_) => anyhow::bail!("EC keys are not supported"),
            _ => anyhow::bail!("unknown PEM item"),
        },
        None => anyhow::bail!("there is no private key in file"),
    }
}
