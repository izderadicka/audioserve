use std::net::SocketAddr;

use futures_util::pin_mut;
use http::Request;
use hyper::{
    body::{Body, Incoming},
    service::Service,
};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto,
};
use tokio::net::TcpListener;

use crate::error::Result;

use self::tls::TlsConfig;

pub mod tls;

pub trait ServiceFactory {
    type Body: Body + Send;
    type Error: Into<Box<dyn std::error::Error + Send + Sync + 'static>>;
    type Future: futures::Future<Output = std::result::Result<http::Response<Self::Body>, Self::Error>>
        + Send;

    type Service: Service<
            Request<Incoming>,
            Response = http::Response<Self::Body>,
            Error = Self::Error,
            Future = Self::Future,
        > + Send;

    fn create(&self, remote_addr: SocketAddr, is_ssl: bool) -> Self::Service;
    fn stop_service_receiver(&self) -> tokio::sync::watch::Receiver<()>;
}
pub struct HttpServer {
    addr: SocketAddr,
}

// pub struct RunningServer;

impl HttpServer {
    pub fn bind(addr: SocketAddr) -> Self {
        Self { addr }
    }

    #[allow(dead_code)]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub async fn serve<S>(
        self,
        service_factory: S,
        #[allow(unused_variables)] tls_config: Option<TlsConfig>,
    ) -> Result<()>
    where
        S: ServiceFactory + Send + 'static,
        S::Body: Body + Send + 'static,
        <<S as ServiceFactory>::Body as Body>::Data: Send,
        <<S as ServiceFactory>::Body as Body>::Error: std::error::Error + Send + Sync + 'static,
    {
        let mut stop_receiver = service_factory.stop_service_receiver();
        let listener = TcpListener::bind(self.addr).await?;

        #[cfg(feature = "tls")]
        let tls_acceptor = tls_config
            .map(|tls_config| self::tls::tls_acceptor(&tls_config))
            .transpose()?;
        let handle = tokio::task::spawn(async move {
            loop {
                let stream;
                let remote_addr;
                tokio::select! {
                    _ = stop_receiver.changed() => {
                        debug!("Stopping server listening loop");
                        break;
                    }

                    res = listener.accept() => {
                        match res {
                            Ok((s, r)) => (stream, remote_addr) = (s, r),
                            Err(e) => {
                                error!("failed to accept connection: {}", e);
                                continue;
                            }
                        };

                    }
                };

                #[cfg(feature = "tls")]
                {
                    let tls_acceptor = tls_acceptor.clone();
                    if let Some(tls_acceptor) = tls_acceptor {
                        match tls_acceptor.accept(stream).await {
                            Ok(stream) => {
                                let io = TokioIo::new(stream);
                                let is_ssl = true;
                                serve_connection(io, &service_factory, remote_addr, is_ssl);
                            }
                            Err(e) => {
                                error!("Failed TLS handshake: {}", e);
                                continue;
                            }
                        }
                    } else {
                        let io = TokioIo::new(stream);
                        let is_ssl = false;
                        serve_connection(io, &service_factory, remote_addr, is_ssl);
                    }
                }

                #[cfg(not(feature = "tls"))]
                {
                    let io = TokioIo::new(stream);
                    let is_ssl = false;
                    serve_connection(io, &service_factory, remote_addr, is_ssl);
                }
            }
        });
        handle.await?;
        Ok(())
    }
}

fn serve_connection<T, S>(io: T, service_factory: &S, remote_addr: SocketAddr, is_ssl: bool)
where
    S: ServiceFactory + Send + 'static,
    S::Body: Body + Send + 'static,
    <<S as ServiceFactory>::Body as Body>::Data: Send,
    <<S as ServiceFactory>::Body as Body>::Error: std::error::Error + Send + Sync + 'static,
    T: hyper::rt::Read + hyper::rt::Write + Send + Unpin + 'static,
{
    let service = service_factory.create(remote_addr, is_ssl);
    let mut stop_signal = service_factory.stop_service_receiver();
    let rt = TokioExecutor::new();
    tokio::task::spawn(async move {
        let builder = auto::Builder::new(rt);
        let conn = builder.serve_connection_with_upgrades(io, service);
        pin_mut!(conn);
        loop {
            tokio::select! {
                _ = stop_signal.changed() => {
                    debug!("Stopping opened connection for {} ", remote_addr);
                    conn.as_mut().graceful_shutdown();

                }
                res = conn.as_mut() => {
                    if let Err(err) = res {
                        error!("Failed to serve connection: {:?}", err);
                    }
                    break;
                }
            }
        }
    });
}
