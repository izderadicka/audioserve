#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate lazy_static;

use collection::audio_folder::FoldersOptions;
use collection::common::CollectionOptions;
use collection::Collections;
use config::{get_config, init_config};
use error::{bail, Context, Error};
use futures::prelude::*;
use hyper::{service::make_service_fn, Server as HttpServer};
use ring::rand::{SecureRandom, SystemRandom};
use services::{
    auth::SharedSecretAuthenticator, search::Search, ServiceFactory, TranscodingDetails,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process;
use std::process::exit;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

mod config;
mod error;
mod services;
#[cfg(feature = "tls")]
mod tls;
mod util;

fn generate_server_secret<P: AsRef<Path>>(file: P) -> Result<Vec<u8>, Error> {
    let file = file.as_ref();
    if file.exists() {
        let mut v = vec![];
        let size = file.metadata().context("secret file metadata")?.len();
        if size > 128 {
            bail!("Secret too long");
        }

        let mut f = File::open(file).context("cannot open secret file")?;
        f.read_to_end(&mut v).context("cannot read secret file")?;
        Ok(v)
    } else {
        let mut random = [0u8; 32];
        let rng = SystemRandom::new();
        rng.fill(&mut random)
            .map_err(|_e| io::Error::new(io::ErrorKind::Other, "Error when generating secret"))?;
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
        Ok(random.to_vec())
    }
}

macro_rules! get_url_path {
    () => {
        get_config()
            .url_path_prefix
            .as_ref()
            .map(|s| s.to_string() + "/")
            .unwrap_or_default()
    };
}

fn create_collections() -> Arc<Collections> {
    let options: HashMap<_, _> = get_config()
        .base_dirs_options
        .iter()
        .map(|(p, o)| {
            (
                p.clone(),
                CollectionOptions {
                    no_cache: o.no_cache,
                },
            )
        })
        .collect();
    Arc::new(
        Collections::new_with_detail::<Vec<PathBuf>, _, _>(
            get_config().base_dirs.clone(),
            options,
            get_config().collections_cache_dir.as_path(),
            FoldersOptions {
                allow_symlinks: get_config().allow_symlinks,
                chapters_duration: get_config().chapters.duration,
                chapters_from_duration: get_config().chapters.from_duration,
                ignore_chapters_meta: get_config().ignore_chapters_meta,
                no_dir_collaps: get_config().no_dir_collaps,
            },
        )
        .expect("Unable to create collections cache"),
    )
}

fn start_server(server_secret: Vec<u8>, collections: Arc<Collections>) -> tokio::runtime::Runtime {
    let cfg = get_config();

    let addr = cfg.listen;
    let start_server = async move {
        let authenticator = get_config().shared_secret.as_ref().map(|secret| {
            SharedSecretAuthenticator::new(secret.clone(), server_secret, cfg.token_validity_hours)
        });
        let transcoding = TranscodingDetails {
            transcodings: Arc::new(AtomicUsize::new(0)),
            max_transcodings: cfg.transcoding.max_parallel_processes,
        };
        let svc_factory = ServiceFactory::new(
            authenticator,
            Search::new(Some(collections.clone())),
            transcoding,
            collections,
            cfg.limit_rate,
        );

        let server: Pin<Box<dyn Future<Output = Result<(), Error>> + Send>> =
            match get_config().ssl.as_ref() {
                None => {
                    let server = HttpServer::bind(&addr).serve(make_service_fn(
                        move |conn: &hyper::server::conn::AddrStream| {
                            let remote_addr = conn.remote_addr();
                            svc_factory.create(Some(remote_addr), false)
                        },
                    ));
                    info!("Server listening on {}{}", &addr, get_url_path!());
                    Box::pin(server.map_err(|e| e.into()))
                }
                Some(ssl) => {
                    #[cfg(feature = "tls")]
                    {
                        use tokio::net::TcpStream;
                        use tokio_native_tls::TlsStream;
                        info!("Server listening on {}{} with TLS", &addr, get_url_path!());
                        let create_server = async move {
                            let incoming = tls::tls_acceptor(&addr, &ssl)
                                .await
                                .context("TLS handshake")?;
                            let server = HttpServer::builder(incoming)
                                .serve(make_service_fn(move |conn: &TlsStream<TcpStream>| {
                                    let remote_addr =
                                        conn.get_ref().get_ref().get_ref().peer_addr().ok();
                                    svc_factory.create(remote_addr, true)
                                }))
                                .await;

                            server.map_err(|e| e.into())
                        };

                        Box::pin(create_server)
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

        server.await
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(cfg.thread_pool.num_threads as usize)
        .max_blocking_threads(cfg.thread_pool.queue_size as usize)
        .build()
        .unwrap();

    rt.spawn(start_server.map_err(|e| error!("Http Server Error: {}", e)));
    rt
}

#[cfg(not(unix))]
async fn terminate_server() {
    use tokio::signal;
    signal::ctrl_c().await.unwrap_or(());
}

#[cfg(unix)]
async fn terminate_server() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt()).expect("Cannot create SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("Cannot create SIGTERM handler");
    let mut sigquit = signal(SignalKind::quit()).expect("Cannot create SIGQUIT handler");

    tokio::select!(
        _ = sigint.recv() => {info!("Terminated on SIGINT")},
        _ = sigterm.recv() => {info!("Terminated on SIGTERM")},
        _ = sigquit.recv() => {info!("Terminated on SIGQUIT")}
    )
}

fn main() {
    #[cfg(unix)]
    {
        if nix::unistd::getuid().is_root() {
            warn!("Audioserve is running as root! Not recommended.")
        }
    }
    if let Err(e) = init_config() {
        eprintln!("Config/Arguments error: {}", e);
        process::exit(1)
    };
    env_logger::init();
    debug!("Started with following config {:?}", get_config());

    collection::init_media_lib();

    #[cfg(feature = "transcoding-cache")]
    {
        use crate::services::transcode::cache::get_cache;
        if get_config().transcoding.cache.disabled {
            info!("Transcoding cache is disabled")
        } else {
            let c = get_cache();
            info!(
                "Using transcoding cache at {:?}, remaining capacity (files,size) : {:?}",
                get_config().transcoding.cache.root_dir,
                c.free_capacity()
            )
        }
    }
    let server_secret = match generate_server_secret(&get_config().secret_file) {
        Ok(s) => s,
        Err(e) => {
            error!("Error creating/reading secret: {}", e);
            process::exit(2)
        }
    };

    let collections = create_collections();

    let runtime = start_server(server_secret, collections.clone());

    runtime.block_on(terminate_server());

    //graceful shutdown of server will wait till transcoding ends, so rather shut it down hard
    runtime.shutdown_timeout(std::time::Duration::from_millis(300));

    thread::spawn(|| {
        thread::sleep(Duration::from_secs(10));
        error!("Forced exit");
        exit(111);
    });

    debug!("Saving collections db");
    match Arc::try_unwrap(collections) {
        Ok(c) => c.close(),
        Err(c) => {
            error!(
                "Cannot close collections, still has {} references",
                Arc::strong_count(&c)
            );
            c.flush().ok(); // flush at least
        }
    }

    #[cfg(feature = "transcoding-cache")]
    {
        if !get_config().transcoding.cache.disabled {
            debug!("Saving transcoding cache");
            use crate::services::transcode::cache::get_cache;
            if let Err(e) = get_cache().save_index_blocking() {
                error!("Error saving transcoding cache index {}", e);
            }
        }
    }

    info!("Server finished");
}
