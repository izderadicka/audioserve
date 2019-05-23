#[macro_use]
extern crate log;
#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;

use config::{get_config, init_config};
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

mod config;
mod error;
mod services;
mod util;

#[cfg(feature = "tls")]
fn load_private_key<P>(file: P, pass: &String) -> Result<Identity, io::Error>
where
    P: AsRef<Path>,
{
    let mut bytes = vec![];
    let mut f = File::open(file)?;
    f.read_to_end(&mut bytes)?;
    let key =
        Identity::from_pkcs12(&bytes, pass).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
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

fn start_server(my_secret: Vec<u8>) -> Result<tokio::runtime::Runtime, Box<std::error::Error>> {
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
            max_transcodings: cfg.transcoding.max_parallel_processes,
        },
    };

    let server: Box<Future<Item = (), Error = ()> + Send> = match get_config().ssl.as_ref() {
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
        Some(ssl) => {
            #[cfg(feature = "tls")]
            {
                use futures::Stream;
                use hyper::server::conn::Http;
                use tokio::net::TcpListener;

                let private_key = match load_private_key(&ssl.key_file, &ssl.key_password) {
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
                    ssl
                )
            }
        }
    };

    let mut rt = tokio::runtime::Builder::new()
        .blocking_threads(cfg.thread_pool.queue_size as usize)
        .core_threads(cfg.thread_pool.num_threads as usize)
        .keep_alive(cfg.thread_pool.keep_alive)
        .name_prefix("tokio-pool-")
        .build()
        .unwrap();

    rt.spawn(server);

    Ok(rt)
}

fn main() {
    #[cfg(unix)]
    {
        if nix::unistd::getuid().is_root() {
            warn!("Audioserve is running as root! Not recommended.")
        }
    }
    match init_config() {
        Err(e) => {
            writeln!(&mut io::stderr(), "Config/Arguments error: {}", e).unwrap();
            process::exit(1)
        }
        Ok(c) => c,
    };
    pretty_env_logger::init();
    debug!("Started with following config {:?}", get_config());

    media_info::init();

    #[cfg(feature = "transcoding-cache")]
    {
        use crate::services::transcode::cache::get_cache;
        if get_config().transcoding.cache.disabled {
            info!("Trascoding cache is disabled")
        } else {
            let c = get_cache();
            info!(
                "Using transcoding cache, remaining capacity (files,size) : {:?}",
                c.free_capacity()
            )
        }
    }
    let my_secret = match gen_my_secret(&get_config().secret_file) {
        Ok(s) => s,
        Err(e) => {
            error!("Error creating/reading secret: {}", e);
            process::exit(2)
        }
    };

    let runtime = match start_server(my_secret) {
        Ok(rt) => rt,
        Err(e) => {
            error!("Error starting server: {}", e);
            process::exit(3)
        }
    };

    #[cfg(unix)]
    {
        use nix::sys::signal;
        let mut sigs = signal::SigSet::empty();
        sigs.add(signal::Signal::SIGINT);
        sigs.add(signal::Signal::SIGQUIT);
        sigs.add(signal::Signal::SIGTERM);
        sigs.thread_block().ok();
        match sigs.wait() {
            Ok(sig) => info!("Terminating by signal {}", sig),
            Err(e) => error!("Signal wait error: {}", e),
        }
        runtime.shutdown_now();

        #[cfg(feature = "transcoding-cache")]
        {
            use crate::services::transcode::cache::get_cache;
            if let Err(e) = get_cache().save_index() {
                error!("Error saving transcoding cache index {}", e);
            }
        }

        crate::services::position::save_positions();
    }

    #[cfg(not(unix))]
    {
        runtime.shutdown_on_idle().wait().unwrap();
    }
    info!("Server finished");
}
