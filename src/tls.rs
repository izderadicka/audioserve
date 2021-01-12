use crate::error::{Context, Error};
use futures::{channel::mpsc, SinkExt};
use hyper::server::accept::{from_stream, Accept};
use native_tls::Identity;
use std::io;
use std::path::Path;
use tokio::{
    fs::File,
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
};
use tokio_native_tls::{TlsAcceptor, TlsStream};

async fn load_private_key<P>(file: P, pass: &str) -> Result<Identity, Error>
where
    P: AsRef<Path>,
{
    let mut bytes = vec![];
    let mut f = File::open(&file)
        .await
        .with_context(|| format!("cannot open private key file {:?}", file.as_ref()))?;
    f.read_to_end(&mut bytes)
        .await
        .context("cannot read private key file")?;
    let key = Identity::from_pkcs12(&bytes, pass).context("invalid private key")?;
    Ok(key)
}

pub(crate) async fn tls_acceptor(
    addr: &std::net::SocketAddr,
    ssl: &crate::config::SslConfig,
) -> Result<impl Accept<Conn = TlsStream<TcpStream>, Error = io::Error>, Error> {
    let private_key = load_private_key(&ssl.key_file, &ssl.key_password).await?;
    let tls_cx = native_tls::TlsAcceptor::builder(private_key)
        .build()
        .context("cannot build native TLS acceptor")?;
    let tls_cx = TlsAcceptor::from(tls_cx);
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot bind address {}", addr))?;

    let (sender, stream) = mpsc::channel(32);

    tokio::spawn(async move {
        loop {
            let s = listener.accept().await;
            match s {
                Ok((s, addr)) => {
                    debug!("Accepted connection from {}", addr);
                    let acceptor = tls_cx.clone();
                    let mut res_sender = sender.clone();
                    tokio::spawn(async move {
                        let conn = acceptor.accept(s).await;
                        match conn {
                            Ok(conn) => {
                                if let Err(e) = res_sender.send(Ok(conn)).await {
                                    error!("internal channel send error: {}", e)
                                };
                            }
                            Err(e) => {
                                error!("Error when accepting TLS connection {}", e);
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                }
            }
        }
    });
    Ok(from_stream(stream))
}

// pub(crate) struct IncommingAcceptor {
//     acceptor: TlsAcceptor,
//     listener: TcpListener,
//     handshake_pending: Vec<>,
//     connected: Vec<TlsStream<TcpStream>>
// }

// impl IncommingAcceptor {
//     pub(crate) async fn new(addr: &std::net::SocketAddr, ssl: &crate::config::SslConfig) -> Result<IncommingAcceptor, Error> {
//         let private_key = load_private_key(&ssl.key_file, &ssl.key_password)?;
//     let tls_cx = native_tls::TlsAcceptor::builder(private_key)
//         .build()
//         .context("cannot build native TLS acceptor")?;
//     let tls_cx = TlsAcceptor::from(tls_cx);
//     let listener = TcpListener::bind(addr)
//         .await
//         .with_context(|| format!("cannot bind address {}", addr))?;

//     }
// }

// impl  Accept for IncommingAcceptor {
//     type Conn = TlsStream<TcpStream>;
//     type Error = io::Error;

//     fn poll_accept(
//         self: Pin<&mut Self>,
//         cx: &mut Context<'_>,
//     ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
//         let pin = self.get_mut();
//         match ready!(Pin::new(&mut pin.incoming).poll_accept(cx)) {
//             Some(Ok(sock)) => {
//                 unimplemented!()}//Poll::Ready(Some(Ok(TlsStream::new(sock, pin.config.clone())))),
//             Some(Err(e)) => Poll::Ready(Some(Err(e))),
//             None => Poll::Ready(None),
//         }
//     }
// }
