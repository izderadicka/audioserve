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
use hyper::server::conn::AddrIncoming;
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
fn load_private_key<P>(file: P, pass: &str) -> Result<Identity, io::Error>
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
        let mut f;
        #[cfg(unix)]
        {
            use std::fs::OpenOptions;
            use std::os::unix::fs::OpenOptionsExt;
            f = OpenOptions::new()
                .mode(0o600)
                .create(true)
                .write(true)
                .truncate(true)
                .open(file)?
        }
        #[cfg(not(unix))]
        {
            f = File::create(file)?
        }
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
    let addr = cfg.listen;
    let incomming_connections = AddrIncoming::bind(&addr)?;

    let server: Box<dyn Future<Item = (), Error = hyper::Error> + Send> =
        match get_config().ssl.as_ref() {
            None => {
                let server = HttpServer::builder(incomming_connections).serve(move || {
                    let s: Result<_, error::Error> = Ok(svc.clone());
                    s
                });
                info!("Server listening on {}", &addr);
                Box::new(server)
            }
            Some(ssl) => {
                #[cfg(feature = "tls")]
                {
                    use futures::Stream;
                    let private_key = match load_private_key(&ssl.key_file, &ssl.key_password) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Error loading SSL/TLS private key: {}", e);
                            return Err(Box::new(e));
                        }
                    };
                    let tls_cx = native_tls::TlsAcceptor::builder(private_key).build()?;
                    let tls_cx = tokio_tls::TlsAcceptor::from(tls_cx);

                    let incoming = incomming_connections
                        .and_then(move |socket| {
                            tls_cx
                                .accept(socket)
                                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                        })
                        // we need to skip TLS errors, so we can accept next connection, otherwise
                        // stream will end and server will stop listening
                        .then(|res| match res {
                            Ok(conn) => Ok::<_, io::Error>(Some(conn)),
                            Err(e) => {
                                error!("TLS error: {}", e);
                                Ok(None)
                            }
                        })
                        .filter_map(|x| x);

                    let server = HttpServer::builder(incoming).serve(move || {
                        let s: Result<_, error::Error> = Ok(svc.clone());
                        s
                    });
                    info!("Server Listening on {} with TLS", &addr);
                    Box::new(server)
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

    let server = server.map_err(|e| error!("Cannot start HTTP server due to error {}", e));

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
        #[cfg(feature = "shared-positions")]
        crate::services::position::save_positions();
    }

    #[cfg(not(unix))]
    {
        runtime.shutdown_on_idle().wait().unwrap();
    }
    info!("Server finished");
}
