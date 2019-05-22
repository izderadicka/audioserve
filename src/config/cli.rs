use super::validators::*;
use super::*;
use clap::{crate_authors, crate_name, crate_version, App, Arg};

type Parser<'a> = App<'a, 'a>;

fn create_parser<'a>(_config: &Config) -> Parser<'a> {
    let mut parser = App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .arg(Arg::with_name("debug")
            .short("d")
            .long("debug")
            .help("Enable debug logging (detailed logging config can be done via RUST_LOG env. variable)")
        )
        .arg(Arg::with_name("local-addr")
            .short("l")
            .long("listen")
            .help("Address and port server is listening on as address:port (by default listen on port 3000 on all interfaces)")
            .takes_value(true)
            .validator(is_socket_addr)
            .env("AUDIOSERVE_LISTEN")
        )
        .arg(Arg::with_name("thread-pool-large")
            .long("thread-pool-large")
            .help("Use larger thread pool (usually will not be needed)")
                   
        )
        .arg(Arg::with_name("thread-pool-keep-alive-secs")
            .long("thread-pool-keep-alive-secs")
            .takes_value(true)
            .help("Threads in pool will shutdown after given seconds, if there is no work. Default is to keep threads forever.")
            .env("AUDIOSERVE_THREAD_POOL_KEEP_ALIVE")
            .validator(is_number)
        )
        .arg(Arg::with_name("base-dir")
            .value_name("BASE_DIR")
            .required(true)
            .multiple(true)
            .min_values(1)
            .max_values(100)
            .takes_value(true)
            .env("AUDIOSERVE_BASE_DIRS")
            .value_delimiter(":")
            .validator_os(is_existing_dir)
            .help("Root directories for audio books, also refered as collections")

        )
        .arg(Arg::with_name("no-authentication")
            .long("no-authentication")
            .help("no authentication required - mainly for testing purposes")
        )
        .arg(Arg::with_name("shared-secret")
            .short("s")
            .long("shared-secret")
            .takes_value(true)
            // .conflicts_with("no-authentication")
            // .required_unless_one(&["no-authentication", "shared-secret-file"])
            .env("AUDIOSERVE_SHARED_SECRET")
            .help("Shared secret for client authentication")
        )
        .arg(Arg::with_name("shared-secret-file")
            .long("shared-secret-file")
            .takes_value(true)
            // .conflicts_with("no-authentication")
            // .required_unless_one(&["no-authentication", "shared-secret"])
            .env("AUDIOSERVE_SHARED_SECRET_FILE")
            .help("File containing shared secret, it's slightly safer to read it from file, then provide as command argument")
        )
        .arg(Arg::with_name("transcoding-max-parallel-processes")
            .short("x")
            .long("transcoding-max-parallel-processes")
            .takes_value(true)
            .validator(is_number)
            .env("AUDIOSERVE_MAX_PARALLEL_PROCESSES")
            .help("Maximum number of concurrent transcoding processes [default: 2 * number of cores]")
        )
        .arg(Arg::with_name("transcoding-max-runtime")
            .long("transcoding-max-runtime")
            .takes_value(true)
            .validator(is_number)
            .env("AUDIOSERVE_TRANSCODING_MAX_RUNTIME")
            .help("Max duration of transcoding process in hours. If takes longer process is killed. Default is 24h")

        )
        .arg(Arg::with_name("token-validity-days")
            .long("token-validity-days")
            .takes_value(true)
            .validator(is_number)
            .env("AUDIOSERVE_TOKEN_VALIDITY_DAYS")
            .help("Validity of authentication token issued by this server in days[default 365, min 10]")
        )
        .arg(Arg::with_name("client-dir")
            .short("c")
            .long("client-dir")
            .takes_value(true)
            .env("AUDIOSERVE_CLIENT_DIR")
            .validator_os(is_existing_dir)
            .help("Directory with client files - index.html and bundle.js")

        )
        .arg(Arg::with_name("secret-file")
            .long("secret-file")
            .takes_value(true)
            .validator_os(parent_dir_exists)
            .env("AUDIOSERVE_SECRET_FILE")
            .help("Path to file where server secret is kept - it's generated if it does not exists [default: is $HOME/.audioserve.secret]")
        )
        .arg(Arg::with_name("cors")
            .long("cors")
            .help("Enable CORS - enabled any origin of requests")
        )
        .arg(Arg::with_name("chapters-from-duration")
            .long("chapters-from-duration")
            .takes_value(true)
            .validator(is_number)
            .env("AUDIOSERVE_CHAPTERS_FROM_DURATION")
            .help("forces split of audio file larger then x mins into chapters (not physically, but it'll be just visible as folder with chapters)[default:0 e.g. disabled]")
        )
        .arg(Arg::with_name("chapters-duration")
            .long("chapters-duration")
            .takes_value(true)
            .validator(is_number)
            .env("AUDIOSERVE_CHAPTERS_FROM_DURATION")
            .help("If long files is presented as chapters, one chapter has x mins [default: 30]")
        );

    if cfg!(feature = "folder-download") {
        parser = parser.arg(
            Arg::with_name("disable-folder-download")
                .long("disable-folder-download")
                .help("Disables API point for downloading whole folder"),
        );
    }

    if cfg!(feature = "tls") {
        parser = parser.arg(Arg::with_name("ssl-key")
            .long("ssl-key")
            .takes_value(true)
            .requires("ssl-key-password")
            .validator_os(is_existing_file)
            .env("AUDIOSERVE_SSL_KEY")
            .help("TLS/SSL private key and certificate in form of PKCS#12 key file, if provided, https is used")
            )
            .arg(Arg::with_name("ssl-key-password")
                .long("ssl-key-password")
                .takes_value(true)
                .requires("ssl-key")
                .env("AUDIOSERVE_SSL_KEY_PASSWORD")
                .help("Password for TLS/SSL private key")
            );
    }

    parser = parser.arg(
        Arg::with_name("positions-file")
            .long("positions-file")
            .takes_value(true)
            .validator_os(parent_dir_exists)
            .env("AUDIOSERVE_POSITIONS_FILE")
            .help("File to save last listened positions"),
    );

    if cfg!(feature = "symlinks") {
        parser = parser.arg(
            Arg::with_name("allow-symlinks")
                .long("allow-symlinks")
                .help("Will follow symbolic/soft links in collections directories"),
        );
    }

    if cfg!(feature = "search-cache") {
        parser=parser.arg(
            Arg::with_name("search-cache")
            .long("search-cache")
            .help("Caches collections directory structure for quick search, monitors directories for changes")
        );
    }

    if cfg!(feature = "transcoding-cache") {
        parser=parser.arg(
            Arg::with_name("t-cache-dir")
            .long("t-cache-dir")
            .takes_value(true)
            .env("AUDIOSERVE_T_CACHE_DIR")
            .validator_os(parent_dir_exists)
            .help("Directory for transcoding cache [default is 'audioserve-cache' under system wide temp dirrectory]")
        ).arg(
            Arg::with_name("t-cache-size")
            .long("t-cache-size")
            .takes_value(true)
            .env("AUDIOSERVE_T_CACHE_SIZE")
            .validator(is_number)
            .help("Max size of transcoding cache in MBi, when reached LRU items are deleted, [default is 1024]")
        ).arg(
            Arg::with_name("t-cache-max-files")
            .long("t-cache-max-files")
            .takes_value(true)
            .env("AUDIOSERVE_T_CACHE_MAX_FILES")
            .validator(is_number)
            .help("Max number of files in transcoding cache, when reached LRU items are deleted, [default is 1024]")
        ).arg(
            Arg::with_name("t-cache-disable")
            .long("t-cache-disable")
            .help("Transaction cache is disabled. If you want to completely get rid of it, compile without 'transcoding-cache'")
            )
        .arg(
            Arg::with_name("t-cache-save-often")
            .long("t-cache-save-often")
            .help("Save additions to cache often, after each addition, this is normaly not necessary")
        )
    }

    parser
}

macro_rules!  arg_error {
    ($arg:expr, $msg:expr) => {
        Error::in_argument($arg, $msg)
    };

    ($arg:expr, $msg:expr, $($param:expr),+) => {
        Error::in_argument($arg,
        format!($msg, $($param),+))
    };

}



pub fn parse_args(mut config: Config) -> Result<Config> {
    let p = create_parser(&config);
    let args = p.get_matches();

    let is_present_or_env = |name: &str, env_name: &str| {
        args.is_present(name) || env::var(env_name).map(|s| s.len()>0).unwrap_or(false)
        
    };

    if args.is_present("debug") {
        let name = "RUST_LOG";
        if env::var_os(name).is_none() {
            env::set_var(name, "debug");
        }
    }

    for dir in args.values_of_os("base-dir").unwrap() {
        
        config.add_base_dir(dir)?;
    }

    if let Ok(port) = env::var("PORT") {
        // this is hack for heroku, which requires program to use env. variable PORT
        let port: u16 = port
            .parse()
            .or_else(|_| arg_error!("local-addr", "Invalid value in $PORT"))?;
        config.local_addr = SocketAddr::from(([0, 0, 0, 0], port));
    } else {
        if let Some(addr) = args.value_of("local-addr") {
            config.local_addr = addr.parse().unwrap();
        }
    }

    if is_present_or_env("thread-pool-large", "AUDIOSERVE_THREAD_POOL_LARGE") {
        config.thread_pool = ThreadPoolConfig {
            num_threads: 16,
            queue_size: 1000,
            keep_alive: None,
        };
    }

    if args.is_present("no-authentication") {
        config.shared_secret = None
    } else if let Some(secret) = args.value_of("shared-secret") {
        config.shared_secret = Some(secret.into())
    } else if let Some(file) = args.value_of_os("shared-secret-file") {
        config.set_shared_secret_from_file(file)?
    } else {
        unreachable!("One of authentcation options must be always present")
    };

    if let Some(n) = args.value_of("transcoding-max-parallel-processes") {
        config.transcoding.max_parallel_processes = n.parse().unwrap()
    }

    if let Some(n) = args.value_of("transcoding-max-runtime") {
        config.transcoding.max_runtime_hours = n.parse().unwrap()
    }

    if let Some(v) = args.value_of("thread-pool-keep-alive-secs") {
        config.thread_pool.keep_alive = Some(Duration::from_secs(v.parse().unwrap()))
    }

    if let Some(validity) = args.value_of("token-validity-days") {
        config.token_validity_hours = validity.parse::<u32>().unwrap() 
    }

    if let Some(client_dir) = args.value_of_os("client-dir") {
        config.client_dir = client_dir.into()
    }
    if let Some(secret_file) = args.value_of_os("secret-file") {
        config.secret_file = secret_file.into()
    }

    if is_present_or_env("cors", "AUDIOSERVE_CORS") {
        config.cors = true;
    }

    if cfg!(feature = "symlinks") && is_present_or_env("allow-symlinks", "AUDIOSERVE_ALLOW_SYMLINKS") {
        config.allow_symlinks = true
    }
    #[cfg(feature = "tls")]
    {
        if let Some(key) = args.value_of("ssl-key") {
            let key_file = key.into();
            let key_password = args.value_of("ssl-key-password").unwrap().into();
            config.ssl = Some(SslConfig {
                key_file,
                key_password,
            });
        }
    }

    if cfg!(feature = "search-cache") && is_present_or_env("search-cache", "AUDIOSERVE_SEARCH_CACHE") {
        config.search_cache = true
    };

    #[cfg(feature = "transcoding-cache")]
    {
        if let Some(d) = args.value_of_os("t-cache-dir") {
            config.transcoding.cache.root_dir=d.into()
        }

        if let Some(n) = args.value_of("t-cache-size") {
            config
                .transcoding.cache 
                .max_size = n.parse().unwrap()
        }

        if let Some(n) = args.value_of("t-cache-max-files") {
            config.transcoding.cache.max_files = n.parse().unwrap()
        }

        if is_present_or_env("t-cache-disable", "AUDIOSERVE_T_CACHE_DISABLE") {
            config.transcoding.cache.disabled = true;
        }

        if is_present_or_env("t-cache-save-often", "AUDIOSERVE_T_CACHE_SAVE_OFTEN") {
            config.transcoding.cache.save_often = true;
        }
    };
    if cfg!(feature = "folder-download") && is_present_or_env("disable-folder-download", "AUDIOSERVE_DISABLE_FOLDER_DOWNLOAD") {
        config.disable_folder_download = true
    };

    if let Some(d) = args.value_of("chapters-from-duration") {
        config.chapters.from_duration = d.parse().unwrap()
    }
      
    if let Some(d) = args.value_of("chapters-duration") {
           config.chapters.duration = d.parse().unwrap()
    }

    if let Some(positions_file) = args.value_of_os("positions-file") {
        config.positions_file = positions_file.into();
    }

    Ok(config)
}
