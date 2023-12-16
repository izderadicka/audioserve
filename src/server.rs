use std::net::SocketAddr;

use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::{error::Result, services::ServiceFactory};

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

    pub async fn serve(self, service_factory: ServiceFactory<()>) -> Result<()> {
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
                tokio::task::spawn(async move {
                    // TODO: support both http1 & http2
                    let conn = http1::Builder::new()
                        .serve_connection(io, service)
                        .with_upgrades();
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
