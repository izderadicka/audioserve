use std::net::SocketAddr;

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

pub trait ServiceFactory<Body> {
    type Error: Into<Box<dyn std::error::Error + Send + Sync + 'static>>;
    type Future: futures::Future<Output = std::result::Result<http::Response<Body>, Self::Error>>
        + Send;

    type Service: Service<
            Request<Incoming>,
            Response = http::Response<Body>,
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

    pub async fn serve<B, S>(self, service_factory: S) -> Result<()>
    where
        S: ServiceFactory<B> + Send + 'static,
        B: Body + Send + 'static,
        B::Data: Send,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        let mut stop_receiver = service_factory.stop_service_receiver();
        let listener = TcpListener::bind(self.addr).await?;
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

                let io = TokioIo::new(stream);
                let is_ssl = false;
                let service = service_factory.create(remote_addr, is_ssl);
                let rt = TokioExecutor::new();
                tokio::task::spawn(async move {
                    let builder = auto::Builder::new(rt);
                    let conn = builder.serve_connection_with_upgrades(io, service);
                    if let Err(err) = conn.await {
                        println!("Failed to serve connection: {:?}", err);
                    }
                });
            }
        });
        handle.await?;
        Ok(())
    }
}
