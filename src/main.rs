#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

// #[cfg(feature = "tls")]
// use crate::tls::TlsStream;
use collection::{CollectionOptions, CollectionOptionsMap, Collections};
use config::{get_config, init_config};
use error::{bail, Context, Error};
use futures::prelude::*;
use ring::rand::{SecureRandom, SystemRandom};
use services::{
    auth::SharedSecretAuthenticator, search::Search, ServiceFactory, TranscodingDetails,
};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::sync::{oneshot, watch};

use crate::server::HttpServer;

mod config;
mod error;
mod server;
mod services;
// #[cfg(feature = "tls")]
// mod tls;
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

fn create_collections_options() -> anyhow::Result<CollectionOptionsMap> {
    let c = get_config();
    let mut fo = CollectionOptions::default();

    fo.allow_symlinks = c.allow_symlinks;
    fo.chapters_duration = c.chapters.duration;
    fo.chapters_from_duration = c.chapters.from_duration;
    fo.ignore_chapters_meta = c.ignore_chapters_meta;
    fo.no_dir_collaps = c.no_dir_collaps;
    fo.tags = c.get_tags();
    fo.cd_folder_regex_str = c.collapse_cd_folders.as_ref().and_then(|x| x.regex.clone());
    fo.force_cache_update_on_init = c.force_cache_update_on_init;
    fo.natural_files_ordering = c.natural_files_ordering;
    fo.time_to_end_of_folder = c.time_to_folder_end;
    fo.read_playlists = c.read_playlist;
    fo.changes_debounce_interval = c.collections_options.changes_debounce_interval;
    fo.watch_for_changes = !c.collections_options.dont_watch_for_changes;

    #[cfg(feature = "tags-encoding")]
    {
        fo.tags_encoding = c.tags_encoding.clone();
    }

    let mut co = CollectionOptionsMap::new(fo)?;
    for (p, o) in &get_config().base_dirs_options {
        if let Err(e) = co.add_col_options(p, o) {
            error!("Invalid option(s) for collection directory {:?}:{}", p, e);
            bail!(e)
        }
    }

    Ok(co)
}

fn create_collections() -> anyhow::Result<Arc<Collections>> {
    let opt = create_collections_options()?;
    Ok(Arc::new(
        Collections::new_with_detail::<Vec<PathBuf>, _, _>(
            get_config().base_dirs.clone(),
            opt,
            get_config().collections_cache_dir.as_path(),
        )
        .expect("Unable to create collections cache"),
    ))
}

#[cfg(feature = "shared-positions")]
fn restore_positions<P: AsRef<Path>>(backup_file: collection::BackupFile<P>) -> anyhow::Result<()> {
    let opt = create_collections_options()?;
    Collections::restore_positions(
        get_config().base_dirs.clone(),
        opt,
        get_config().collections_cache_dir.as_path(),
        backup_file,
    )
    .map_err(Error::new)
}

fn start_server(
    server_secret: Vec<u8>,
    collections: Arc<Collections>,
) -> (
    tokio::runtime::Runtime,
    oneshot::Receiver<()>,
    watch::Sender<()>,
) {
    let cfg = get_config();

    let addr = cfg.listen;
    let (stop_service_sender, stop_service_receiver) = watch::channel(());
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
            stop_service_receiver,
        );

        let server: Pin<Box<dyn Future<Output = Result<(), Error>> + Send>> =
            match get_config().ssl.as_ref() {
                None => {
                    let server = HttpServer::bind(addr).serve(svc_factory);
                    info!("Server listening on {}{}", addr, get_url_path!());
                    Box::pin(server.map_err(|e| e.into()))
                }
                Some(_ssl) => {
                    #[cfg(feature = "tls")]
                    todo!("Implement TLS server");
                    // {
                    //     info!("Server listening on {}{} with TLS", &addr, get_url_path!());
                    //     let create_server = async move {
                    //         let incoming = tls::tls_acceptor(addr, ssl)?;
                    //         let server = HttpServer::builder(incoming)
                    //             .serve(make_service_fn(move |conn: &TlsStream| {
                    //                 let remote_addr = conn.remote_addr();
                    //                 svc_factory.create(remote_addr, true)
                    //             }))
                    //             .await;

                    //         server.map_err(|e| e.into())
                    //     };

                    //     Box::pin(create_server)
                    // }

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

    let (term_sender, term_receiver) = oneshot::channel();
    rt.spawn(
        start_server
            .map_err(|e| error!("Http Server Error: {}", e))
            .then(move |_| {
                term_sender.send(()).ok();
                futures::future::ready(())
            }),
    );
    (rt, term_receiver, stop_service_sender)
}

#[cfg(not(unix))]
async fn terminate_server() {
    use tokio::signal;
    signal::ctrl_c().await.unwrap_or(());
}

#[cfg(unix)]
async fn terminate_server(
    term_receiver: oneshot::Receiver<()>,
    stop_service_sender: watch::Sender<()>,
) {
    use tokio::signal::unix::{signal, SignalKind};
    const SIGINT: SignalKind = SignalKind::interrupt();
    const SIGTERM: SignalKind = SignalKind::terminate();
    const SIGQUIT: SignalKind = SignalKind::quit();

    let mut sigint = signal(SIGINT).expect("Cannot create SIGINT handler");
    let mut sigterm = signal(SIGTERM).expect("Cannot create SIGTERM handler");
    let mut sigquit = signal(SIGQUIT).expect("Cannot create SIGQUIT handler");

    let terminate_on_signal = move |signal: SignalKind| {
        let signal_name = match signal {
            SIGINT => "SIGINT",
            SIGTERM => "SIGTERM",
            SIGQUIT => "SIGQUIT",
            _ => "Other",
        };
        info!("Terminated on {}", signal_name);
        stop_service_sender.send(()).ok();
    };

    tokio::select!(
        _ = sigint.recv() => terminate_on_signal(SIGINT),
        _ = sigterm.recv() => terminate_on_signal(SIGTERM),
        _ = sigquit.recv() => terminate_on_signal(SIGQUIT),
        _ = term_receiver => {warn!("Terminated because HTTP server finished unexpectedly")}
    )
}

#[cfg(unix)]
async fn watch_for_cache_update_signal(cols: Arc<Collections>) {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigusr1 = signal(SignalKind::user_defined1()).expect("Cannot create SIGUSR1 handler");
    while let Some(()) = sigusr1.recv().await {
        info!("Received signal SIGUSR1 for full rescan of caches");
        cols.clone().force_rescan()
    }
}

#[cfg(unix)]
#[cfg(feature = "shared-positions")]
async fn watch_for_positions_backup_signal(cols: Arc<Collections>) {
    use tokio::signal::unix::{signal, SignalKind};
    let mut cron = get_config()
        .positions
        .backup_schedule
        .as_ref()
        .map(|s| crate::util::parse_cron(s).expect("invalid cron expression"));
    let mut next_dur = move || {
        cron.as_mut()
            .and_then(|cron| cron.upcoming(chrono::Local).next())
            .map(|d| {
                (d - chrono::Local::now())
                    .to_std()
                    .unwrap_or_else(|_| Duration::from_millis(100))
            })
            .unwrap_or_else(|| Duration::from_secs(u64::MAX))
    };
    let mut sigusr2 = signal(SignalKind::user_defined2()).expect("Cannot create SIGUSR2 handler");

    loop {
        let res = tokio::time::timeout(next_dur(), sigusr2.recv()).await;
        match res {
            Ok(None) => break,
            Ok(Some(())) => info!("Received signal SIGUSR2 for positions backup"),
            Err(_) => debug!("scheduled positions backup"),
        }
        if let Some(backup_file) = get_config().positions.backup_file.as_ref() {
            cols.clone()
                .backup_positions_async(backup_file)
                .await
                .map_err(|e| error!("Backup of positions failed: {}", e))
                .ok();
        } else {
            error!("Positions backup file not configured")
        }
    }
}

fn main() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        if nix::unistd::geteuid().is_root() {
            warn!("Audioserve is running as root! Not recommended.")
        }
    }
    if let Err(e) = init_config() {
        return Err(Error::msg(format!("Config/Arguments error: {}", e)));
    };
    env_logger::init();
    info!(
        "Started audioserve {} with features {}",
        config::LONG_VERSION,
        config::FEATURES
    );
    if log_enabled!(log::Level::Debug) {
        let mut cfg = get_config().clone();
        cfg.shared_secret = cfg.shared_secret.map(|_| "******".to_string()); // Do not want to write secret to log!
        debug!("Started with following config {:?}", cfg);
    }

    collection::init_media_lib();

    #[cfg(feature = "shared-positions")]
    if !matches!(
        get_config().positions.restore,
        config::PositionsBackupFormat::None
    ) {
        let backup_file = get_config()
            .positions
            .backup_file
            .clone()
            .expect("Missing backup file argument");

        use collection::BackupFile;
        use config::PositionsBackupFormat::*;
        let backup_file = match get_config().positions.restore {
            None => unreachable!(),
            Legacy => BackupFile::Legacy(backup_file),
            V1 => BackupFile::V1(backup_file),
        };

        restore_positions(backup_file).context("Error while restoring position")?;

        let msg =
            "Positions restoration is finished, exiting program, restart it now without --positions-restore arg";
        info!("{}", msg);
        println!("{}", msg);
        return Ok(());
    }

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
        Err(e) => return Err(Error::msg(format!("Error creating/reading secret: {}", e))),
    };

    let collections = create_collections()?;

    let (runtime, term_receiver, stop_service_sender) =
        start_server(server_secret, collections.clone());

    #[cfg(unix)]
    {
        runtime.spawn(watch_for_cache_update_signal(collections.clone()));
        #[cfg(feature = "shared-positions")]
        runtime.spawn(watch_for_positions_backup_signal(collections.clone()));
    }

    runtime.block_on(terminate_server(term_receiver, stop_service_sender));

    //graceful shutdown of server will wait till transcoding ends, so rather shut it down hard
    runtime.shutdown_timeout(std::time::Duration::from_millis(300));

    thread::spawn(|| {
        const FINISH_LIMIT: u64 = 10;
        thread::sleep(Duration::from_secs(FINISH_LIMIT));
        error!(
            "Forced exit, program is not finishing with limit of {}s",
            FINISH_LIMIT
        );
        process::exit(111);
    });

    debug!("Saving collections db");
    match Arc::try_unwrap(collections) {
        Ok(c) => drop(c),
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

    if !get_config().icons.cache_disabled {
        if let Err(e) = crate::services::icon::cache::get_cache().save_index() {
            error!("Error saving icons cache index {}", e);
        }
    }

    info!("Server finished");

    Ok(())
}
