use clap::{crate_authors, crate_name, crate_version, App, Arg};
use super::*;
use super::validators::*;

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
            .env("AUDIOSERVE_THREAD_POOL_LARGE")        
        )
        .arg(Arg::with_name("thread-pool-keep-alive")
            .long("thread-pool-keep-alive")
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
            .env("AUDIOSERVE_NO_AUTHENTICATION")
            .help("no authentication required - mainly for testing purposes")
        )
        .arg(Arg::with_name("shared-secret")
            .short("s")
            .long("shared-secret")
            .takes_value(true)
            .conflicts_with("no-authentication")
            .required_unless_one(&["no-authentication", "shared-secret-file"])
            .env("AUDIOSERVE_SHARED_SECRET")
            .help("Shared secret for client authentication")
        )
        .arg(Arg::with_name("shared-secret-file")
            .long("shared-secret-file")
            .takes_value(true)
            .conflicts_with("no-authentication")
            .required_unless("shared-secret")
            .env("AUDIOSERVE_SHARED_SECRET_FILE")
            .help("File containing shared secret, it's slightly safer to read it from file, then provide as command argument")
        )
        
        .arg(Arg::with_name("max-transcodings")
            .short("x")
            .long("max-transcodings")
            .takes_value(true)
            .help("Maximum number of concurrent transcodings [default: 2 * number of cores]")
        )
        .arg(Arg::with_name("transcoding-deadline")
            .long("transcoding-deadline")
            .takes_value(true)
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
            .env("AUDIOSERVE_CORS")
            .help("Enable CORS - enabled any origin of requests")
        )
        .arg(Arg::with_name("chapters-from-duration")
            .long("chapters-from-duration")
            .takes_value(true)
            .help("forces split of audio file larger then x mins into chapters (not physically, but it'll be just visible as folder with chapters)[default:0 e.g. disabled]")
        )
        .arg(Arg::with_name("chapters-duration")
            .long("chapters-duration")
            .takes_value(true)
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
        Arg::with_name("positions-dir")
            .long("positions-dir")
            .takes_value(true)
            .help("Directory to save last listened positions"),
    );

    if cfg!(feature = "symlinks") {
        parser = parser.arg(
            Arg::with_name("allow-symlinks")
                .long("allow-symlinks")
                .env("AUDIOSERVE_ALLOW_SYMLINKS")
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
            .help("Directory for transcoding cache [default is 'audioserve-cache' under system wide temp dirrectory]")
        ).arg(
            Arg::with_name("t-cache-size")
            .long("t-cache-size")
            .takes_value(true)
            .help("Max size of transcoding cache in MBi, when reached LRU items are deleted, [default is 1024]")
        ).arg(
            Arg::with_name("t-cache-max-files")
            .long("t-cache-max-files")
            .takes_value(true)
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

pub fn parse_args(mut config:Config) -> Result<Config> {
    
    let p = create_parser(&config);
    let args = p.get_matches();

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
        let port: u16 = port.parse()
        .or_else(|_| arg_error!("local-addr", "Invalid value in $PORT"))?;
        config.local_addr = SocketAddr::from((
            [0, 0, 0, 0],port));

    } else {
        if let Some(addr) = args.value_of("local-addr") {
            config.local_addr = addr.parse().unwrap();
        }
    }

    
    if args.is_present("thread-pool-large") {
        config.set_pool_size(
        ThreadPoolConfig {
            num_threads: 16,
            queue_size: 1000,
            keep_alive: None
        })?;
    } 

    if args.is_present("no-authentication") {
        config.shared_secret = None
    } else if args.is_present("shared-secret") {
        config.set_shared_secret(args.value_of("shared-secret").unwrap().into())?
    } else if args.is_present("shared-secret-file") {
        config.set_shared_secret_from_file(args.value_of_os("shared-secret-file").unwrap())?
        
    } else {
        unreachable!("One of authentcation options must be always present")
    };

    //TODO - refactor transcoding config
    if let Some(s) = args.value_of("max-transcodings") {
        let max_transcodings = s.parse()
            .or_else(|e| arg_error!("max-transcodings", "Invalid value: {}", e))?;
    
        if max_transcodings < 1 {
            return arg_error!("max-transcodings", "At least one concurrent trancoding must be available");
        } else if max_transcodings > 100 {
            return arg_error!("max_transcodings",
                "As transcodings are resource intesive, having more then 100 is not wise"
            );
        }

        config.max_transcodings = max_transcodings;

    }

    match args.value_of("transcoding-deadline").map(str::parse) {
        Some(Ok(0)) => return arg_error!("transcoding-deadline", 
        "value must be positive"),
        Some(Err(e)) => return arg_error!("transcoding-deadline",
            "invalid value : {}", e),
        Some(Ok(x)) => config.transcoding_deadline = x,
        None => (),
    };

    if args.is_present("thread-pool-keep-alive"){
        let v = args.value_of("thread-pool-keep-alive").unwrap();
        config.thread_pool.set_keep_alive_secs(v.parse().unwrap())?
    }

    if  args.is_present("token-validity-days") {
        let validity = args.value_of("token-validity-days").unwrap().parse().unwrap();
        config.set_token_validity_days(validity)?
    }

    if args.is_present("client-dir") {
        config.set_client_dir(args.value_of_os("client-dir").unwrap())?
    }
    if args.is_present("secret-file") {
        let secret_file = args.value_of("secret-file").unwrap();
        config.set_secret_file(secret_file)?
    }

    if args.is_present("cors") {
        config.cors = true;
    }
    
    if cfg!(feature = "symlinks") && args.is_present("allow-symlinks") {
            config.allow_symlinks = true
        }
    #[cfg(feature = "tls")]
    {
        if  args.is_present("ssl-key") {
            let key_file = args.value_of("ssl-key").unwrap().into();
            let key_password = args.value_of("ssl-key-password").unwrap().into();
            config.set_ssl_config(SslConfig{key_file, key_password})?

        }
        
    }

    if cfg!(feature = "search-cache") &&
        args.is_present("search-cache") {
        config.search_cache = true
    };

    #[cfg(feature = "transcoding-cache")]
    let _transcoding_cache = {
        let mut c = TranscodingCacheConfig::default();
        if let Some(d) = args.value_of("t-cache-dir") {
            c.root_dir = d.into()
        }

        if let Some(n) = args.value_of("t-cache-size") {
            let size: u64 = n.parse().unwrap();
            if size < 50 {
                //return Err("Cache smaller then 50Mbi does not make much sense".into());
            }
            c.max_size = 1024 * 1024 * size;
        }

        if let Some(n) = args.value_of("t-cache-max-files") {
            let num: u64 = n.parse().unwrap();
            if num < 10 {
                //return Err("Cache smaller then 10 files does not make much sense".into());
            }
            c.max_files = num;
        }

        if args.is_present("t-cache-disable") {
            c.disabled = true;
        }

        if args.is_present("t-cache-save-often") {
            c.save_often = true;
        }

        c
    };

    let disable_folder_download = if cfg!(feature = "folder-download") {
        args.is_present("disable-folder-download")
    } else {
        true
    };

    let chapters = {
        let mut c = ChaptersSize::default();
        let from_duration = args
            .value_of("chapters-from-duration")
            .and_then(|v| v.parse().ok());
        if let Some(from_duration) = from_duration {
            c.from_duration = from_duration
        }

        let duration = args
            .value_of("chapters-duration")
            .and_then(|v| v.parse().ok());
        if let Some(duration) = duration {
            if duration < 10 {
                // return Err(Error::InvalidLimitValue(
                //     "chapter should have at least 10 mins",
                // ));
            }
            c.duration = duration;
        }

        c
    };

    let positions_file = {
        match dirs::home_dir() {
            Some(home) => home.join(".audioserve-positions"),
            None => "./audioserve-positions".into(),
        }
    };

    // let config = Config {
    //     base_dirs,
    //     local_addr,
    //     pool_size,
    //     shared_secret,
    //     transcoding,
    //     max_transcodings,
    //     token_validity_hours: token_validity_days * 24,
    //     client_dir,
    //     secret_file,
    //     cors,
    //     ssl_key_file,
    //     ssl_key_password,
    //     allow_symlinks,
    //     thread_keep_alive,
    //     transcoding_deadline,
    //     search_cache,
    //     #[cfg(feature = "transcoding-cache")]
    //     transcoding_cache: _transcoding_cache,
    //     disable_folder_download,
    //     chapters,
    //     positions_file,
    // };

    Ok(config)
}