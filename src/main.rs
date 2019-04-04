#[macro_use]
extern crate log;
#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;

use config::{get_config, parse_args};
use hyper::rt::Future;
use hyper::Server as HttpServer;
use ring::rand::{SecureRandom, SystemRandom};
use services::auth::SharedSecretAuthenticator;
use services::search::Search;
use services::{FileSendService, TranscodingDetails};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

#[cfg(feature = "tls")]
use native_tls::Identity;

mod util;
mod config;
mod error;
mod services;
#[cfg(feature = "transcoding-cache")]
mod cache;

#[cfg(feature = "tls")]
fn load_private_key<P>(file: P, pass: Option<&String>) -> Result<Identity, io::Error>
where
    P: AsRef<Path>,
{
    let mut bytes = vec![];
    let mut f = File::open(file)?;
    f.read_to_end(&mut bytes)?;
    let key = Identity::from_pkcs12(&bytes, pass.unwrap_or(&String::new()))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(key)
}

fn gen_my_secret<P: AsRef<Path>>(file: P) -> Result<Vec<u8>, io::Error> {
    let file = file.as_ref();
    if file.exists() {
        let mut v = vec![];
        let size = file.metadata()?.len();
        if size > 128 {
            return Err(io::Error::new(io::ErrorKind::Other, "Secret too long"));
        }

        let mut f = File::open(file)?;
        f.read_to_end(&mut v)?;
        Ok(v)
    } else {
        let mut random = [0u8; 32];
        let rng = SystemRandom::new();
        rng.fill(&mut random)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let mut f = File::create(file)?;
        f.write_all(&random)?;
        Ok(random.iter().cloned().collect())
    }
}

fn start_server(my_secret: Vec<u8>) -> Result<(), Box<std::error::Error>> {
    let cfg = get_config();
    let svc = FileSendService {
        authenticator: get_config().shared_secret.as_ref().map(
            |secret| -> Arc<Box<services::auth::Authenticator<Credentials = ()>>> {
                Arc::new(Box::new(SharedSecretAuthenticator::new(
                    secret.clone(),
                    my_secret,
                    cfg.token_validity_hours,
                )))
            },
        ),
        search: Search::new(),
        transcoding: TranscodingDetails {
            transcodings: Arc::new(AtomicUsize::new(0)),
            max_transcodings: cfg.max_transcodings,
        },
    };

    let server: Box<Future<Item = (), Error = ()> + Send> = match get_config().ssl_key_file.as_ref()
    {
        None => {
            let server = HttpServer::bind(&get_config().local_addr)
                .serve(move || {
                    let s: Result<_, error::Error> = Ok(svc.clone());
                    s
                })
                .map_err(|e| error!("Cannot start HTTP server due to error {}", e));
            info!("Server listening on {}", &get_config().local_addr);
            Box::new(server)
        }
        Some(file) => {
            #[cfg(feature = "tls")]
            {
                use futures::Stream;
                use hyper::server::conn::Http;
                use native_tls;
                use tokio::net::TcpListener;
                use tokio_tls;

                let private_key =
                    match load_private_key(file, get_config().ssl_key_password.as_ref()) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Error loading SSL/TLS private key: {}", e);
                            return Err(Box::new(e));
                        }
                    };
                let tls_cx = native_tls::TlsAcceptor::builder(private_key).build()?;
                let tls_cx = tokio_tls::TlsAcceptor::from(tls_cx);

                let addr = cfg.local_addr;
                let srv = TcpListener::bind(&addr)?;
                let http_proto = Http::new();
                let http_server = http_proto
                    .serve_incoming(
                        srv.incoming().and_then(move |socket| {
                            tls_cx
                                .accept(socket)
                                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                        }),
                        move || {
                            let s: Result<_, error::Error> = Ok(svc.clone());
                            s
                        },
                    )
                    .then(|res| match res {
                        Ok(conn) => Ok(Some(conn)),
                        Err(e) => {
                            error!("TLS error: {}", e);
                            Ok(None)
                        }
                    })
                    .for_each(|conn_opt| {
                        if let Some(conn) = conn_opt {
                            tokio::spawn(
                                conn.and_then(|c| c.map_err(error::Error::new_with_cause))
                                    .map_err(|e| error!("Connection error {}", e)),
                            );
                        }

                        Ok(())
                    });
                info!("Server Listening on {} with TLS", addr);
                Box::new(http_server)
            }

            #[cfg(not(feature = "tls"))]
            {
                panic!(
                    "TLS is not compiled - build with default features {:?}",
                    file
                )
            }
        }
    };

    // let mut builder = tokio_threadpool::Builder::new();
    // builder.keep_alive(
    //     cfg.thread_keep_alive
    //         .map(|secs| std::time::Duration::from_secs(u64::from(secs))),
    // );
    let mut rt = tokio::runtime::Builder::new()
        .blocking_threads(cfg.pool_size.queue_size)
        .core_threads(cfg.pool_size.num_threads)
        .name_prefix("tokio-pool-")
        //.keep_alive()
        .build()
        .unwrap();

    rt.spawn(server);
    rt.shutdown_on_idle().wait().unwrap();

    Ok(())
}

fn main() {
    match parse_args() {
        Err(e) => {
            writeln!(&mut io::stderr(), "Arguments error: {}", e).unwrap();
            process::exit(1)
        }
        Ok(c) => c,
    };
    pretty_env_logger::init();
    debug!("Started with following config {:?}", get_config());
    
    media_info::init();
    
    #[cfg(feature="transcoding-cache")]
    {
        use cache::get_cache;
        if get_config().transcoding_cache.disabled {
            info!("Trascoding cache is disabled")
        } else {
            let c = get_cache();
            info!("Using transcoding cache, remaining capacity (files,size) : {:?}", c.free_capacity())
        }
    }
    let my_secret = match gen_my_secret(&get_config().secret_file) {
        Ok(s) => s,
        Err(e) => {
            error!("Error creating/reading secret: {}", e);
            process::exit(2)
        }
    };

    match start_server(my_secret) {
        Ok(_) => (),
        Err(e) => {
            error!("Error starting server: {}", e);
            process::exit(3)
        }
    }

    info!("Server finished");
}
