use crate::error::{Context, Error};
use async_stream::try_stream;
use hyper::server::accept::{from_stream, Accept};
use native_tls::Identity;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use tokio::net::{TcpListener, TcpStream};
use tokio_native_tls::{TlsAcceptor, TlsStream};

fn load_private_key<P>(file: P, pass: &str) -> Result<Identity, Error>
where
    P: AsRef<Path>,
{
    let mut bytes = vec![];
    let mut f = File::open(&file)
        .with_context(|| format!("cannot open private key file {:?}", file.as_ref()))?;
    f.read_to_end(&mut bytes)
        .context("cannot read private key file")?;
    let key = Identity::from_pkcs12(&bytes, pass).context("invalid private key")?;
    Ok(key)
}

pub(crate) async fn tls_acceptor(
    addr: &std::net::SocketAddr,
    ssl: &crate::config::SslConfig,
) -> Result<impl Accept<Conn = TlsStream<TcpStream>, Error = io::Error>, Error> {
    let private_key = load_private_key(&ssl.key_file, &ssl.key_password)?;
    let tls_cx = native_tls::TlsAcceptor::builder(private_key)
        .build()
        .context("cannot build native TLS acceptor")?;
    let tls_cx = TlsAcceptor::from(tls_cx);
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot bind address {}", addr))?;

    let stream = try_stream! {
            loop {
                let s =  listener.accept().await;
                match s {
                    Ok((s, addr)) => {
                        debug!("Accepted connection from {}", addr);
                        let acceptor = tls_cx.clone();
                        let conn = acceptor.accept(s).await;
                        match conn {
                            Ok(conn) => {
                                yield conn
                    }
                    Err(e) => {
                        error!("Error when accepting TLS connection {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
            }
        }
    };

    Ok(from_stream(stream))
}

// pub(crate) struct TlsAcceptor {
//     acceptor: TokioTlsAcceptor,
//     incoming: AddrIncoming,
// }

// impl TlsAcceptor {
//     pub(crate) fn new(incoming: AddrIncoming, ssl: &crate::config::SslConfig) -> Result<TlsAcceptor, Error> {
//         let private_key = load_private_key(&ssl.key_file, &ssl.key_password)?;
//         let tls_cx = native_tls::TlsAcceptor::builder(private_key).build()?;
//         let tls_cx = tokio_tls::TlsAcceptor::from(tls_cx);
//         Ok(
//         TlsAcceptor {
//             incoming,
//             acceptor: tls_cx
//         }
//     )
//     }
// }

// impl  Accept for TlsAcceptor {
//     type Conn = TlsStream<AddrStream>;
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
