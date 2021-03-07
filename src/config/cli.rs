use super::validators::*;
use super::*;
use clap::{crate_authors, crate_name, crate_version, App, Arg};

type Parser<'a> = App<'a, 'a>;

fn create_parser<'a>() -> Parser<'a> {
    let mut parser = App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .arg(Arg::with_name("config")
            .short("g")
            .long("config")
            .takes_value(true)
            .env("AUDIOSERVE_CONFIG")
            .validator_os(is_existing_file)
            .help("Configuration file in YAML format")
            )
        .arg(Arg::with_name("print-config")
            .long("print-config")
            .help("Will print current config, with all other options to stdout, usefull for creating config file")
            )
        .arg(Arg::with_name("data-dir")
            .long("data-dir")
            .takes_value(true)
            .validator_os(parent_dir_exists)
            .env("AUDIOSERVE_DATA_DIR")
            .help("Base directory for data created by audioserve (caches, state, ...) [default is $HOME/.audioserve]")
            )
        .arg(Arg::with_name("debug")
            .short("d")
            .long("debug")
            .help("Enable debug logging (detailed logging config can be done via RUST_LOG env. variable)")
            )
        .arg(Arg::with_name("listen")
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
            .multiple(true)
            .min_values(1)
            .max_values(100)
            .takes_value(true)
            .env("AUDIOSERVE_BASE_DIRS")
            .value_delimiter(":")
            .validator_os(is_existing_dir)
            .help("Root directories for audio books, also referred as collections")

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
        .arg(Arg::with_name("limit-rate")
            .long("limit-rate")
            .env("AUDIOSERVE_LIMIT_RATE")
            .takes_value(true)
            .validator(is_positive_float)
            .help("Limits number of http request to x req/sec. Assures that resources are not exhausted in case of DDoS (but will also limit you). It's bit experimental now.")
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
            )
        .arg(Arg::with_name("no-dir-collaps")
            .long("no-dir-collaps")
            .help("Prevents automatic collaps/skip of directory with single chapterized audio file")

            )
        .arg(Arg::with_name("ignore-chapters-meta")
            .long("ignore-chapters-meta")
            .help("Ignore chapters metadata, so files with chapters will not be presented as folders")
            )
        .arg(Arg::with_name("url-path-prefix")
        .long("url-path-prefix")
        .takes_value(true)
        .validator(is_valid_url_path_prefix)
        .env("AUDIOSERVE_URL_PATH_PREFIX")
        .help("Base URL is a fixed path that is before audioserve path part, must start with / and not end with /  [default: none]")
            );

    if cfg!(feature = "behind-proxy") {
        parser = parser.arg(Arg::with_name("behind-proxy")
        .long("behind-proxy")
        .help("Informs program that it is behind remote proxy, now used only for logging (to get true remote client ip)")
        )
    }

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

    if cfg!(feature = "shared-positions") {
        parser = parser.arg(
        Arg::with_name("positions-file")
            .long("positions-file")
            .takes_value(true)
            .validator_os(parent_dir_exists)
            .env("AUDIOSERVE_POSITIONS_FILE")
            .help("File to save last listened positions []"),
        )
        .arg(
            Arg::with_name("positions-ws-timeout")
            .long("positions-ws-timeout")
            .validator(is_number)
            .env("AUDIOSERVE_POSITIONS_WS_TIMEOUT")
            .help("Timeout in seconds for idle websocket connection use for playback position sharing [default 600s]")
        );
    }

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
            .help("Directory for transcoding cache [default is ~/.audioserve/audioserve-cache]")
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
            .conflicts_with_all(&["t-cache-save-often", "t-cache-max-files", "t-cache-size", "t-cache-dir"])
            .help("Transaction cache is disabled. If you want to completely get rid of it, compile without 'transcoding-cache'")
            )
        .arg(
            Arg::with_name("t-cache-save-often")
            .long("t-cache-save-often")
            .help("Save additions to cache often, after each addition, this is normally not necessary")
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

pub fn parse_args() -> Result<Config> {
    parse_args_from(env::args_os())
}

// Although function  is bit too long it does not make sense to split, as it deals with each config option in very plain matter
#[allow(clippy::cognitive_complexity)]
pub fn parse_args_from<I, T>(args: I) -> Result<Config>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let p = create_parser();
    let args = p.get_matches_from(args);

    if let Some(dir) = args.value_of_os("data-dir") {
        unsafe {
            BASE_DATA_DIR.take();
            BASE_DATA_DIR = Some(dir.into());
        }
    }

    //Assure that BASE_DATA_DIR exists
    {
        let d = base_data_dir();
        if !d.is_dir() {
            std::fs::create_dir(&d).or_else(|e| {
                arg_error!(
                    "data-dir",
                    "Audioserve data directory {:?} cannot be created due to error {}",
                    d,
                    e
                )
            })?
        }
    }

    let mut no_authentication_confirmed = false;

    let mut config: Config = if let Some(config_file) = args.value_of_os("config") {
        let f = File::open(config_file).or_else(|e| {
            arg_error!(
                "config",
                "Cannot open config file {:?}, error: {}",
                config_file,
                e
            )
        })?;

        serde_yaml::from_reader(f).or_else(|e| {
            arg_error!(
                "config",
                "Invalid config file {:?}, error: {}",
                config_file,
                e
            )
        })?
    } else {
        Config::default()
    };

    let is_present_or_env = |name: &str, env_name: &str| {
        args.is_present(name) || env::var(env_name).map(|s| !s.is_empty()).unwrap_or(false)
    };

    if args.is_present("debug") {
        let name = "RUST_LOG";
        if env::var_os(name).is_none() {
            env::set_var(name, "debug");
        }
    }

    if let Some(base_dirs) = args.values_of_os("base-dir") {
        for dir in base_dirs {
            config.add_base_dir(dir)?;
        }
    }

    if let Ok(port) = env::var("PORT") {
        // this is hack for heroku, which requires program to use env. variable PORT
        let port: u16 = port
            .parse()
            .or_else(|_| arg_error!("listen", "Invalid value in $PORT"))?;
        config.listen = SocketAddr::from(([0, 0, 0, 0], port));
    } else if let Some(addr) = args.value_of("listen") {
        config.listen = addr.parse().unwrap();
    }

    if is_present_or_env("thread-pool-large", "AUDIOSERVE_THREAD_POOL_LARGE") {
        config.thread_pool = ThreadPoolConfig {
            num_threads: 16,
            queue_size: 1000,
            keep_alive: None,
        };
    }

    if args.is_present("no-authentication") {
        config.shared_secret = None;
        no_authentication_confirmed = true
    } else if let Some(secret) = args.value_of("shared-secret") {
        config.shared_secret = Some(secret.into())
    } else if let Some(file) = args.value_of_os("shared-secret-file") {
        config.set_shared_secret_from_file(file)?
    };

    if let Some(r) = args.value_of("limit-rate").and_then(|s| s.parse().ok()) {
        config.limit_rate = Some(r)
    }

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
        config.token_validity_hours = validity.parse::<u32>().unwrap() * 24
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

    if cfg!(feature = "symlinks")
        && is_present_or_env("allow-symlinks", "AUDIOSERVE_ALLOW_SYMLINKS")
    {
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

    if cfg!(feature = "search-cache")
        && is_present_or_env("search-cache", "AUDIOSERVE_SEARCH_CACHE")
    {
        config.search_cache = true
    };

    #[cfg(feature = "transcoding-cache")]
    {
        if let Some(d) = args.value_of_os("t-cache-dir") {
            config.transcoding.cache.root_dir = d.into()
        }

        if let Some(n) = args.value_of("t-cache-size") {
            config.transcoding.cache.max_size = n.parse().unwrap()
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
    if cfg!(feature = "folder-download")
        && is_present_or_env(
            "disable-folder-download",
            "AUDIOSERVE_DISABLE_FOLDER_DOWNLOAD",
        )
    {
        config.disable_folder_download = true
    };

    if is_present_or_env("behind-proxy", "AUDIOSERVE_BEHIND_PROXY") {
        config.behind_proxy = true;
    }

    if let Some(d) = args.value_of("chapters-from-duration") {
        config.chapters.from_duration = d.parse().unwrap()
    }

    if let Some(d) = args.value_of("chapters-duration") {
        config.chapters.duration = d.parse().unwrap()
    }

    if is_present_or_env("no-dir-collaps", "AUDIOSERVE_NO_DIR_COLLAPS") {
        config.no_dir_collaps = true;
    }

    if is_present_or_env("ignore-chapters-meta", "AUDIOSERVE_IGNORE_CHAPTERS_META") {
        config.ignore_chapters_meta = true;
    }

    if let Some(positions_file) = args.value_of_os("positions-file") {
        config.positions_file = positions_file.into();
    }

    if let Some(positions_ws_timeout) = args.value_of("positions-ws-timeout") {
        config.positions_ws_timeout = Duration::from_secs(positions_ws_timeout.parse().unwrap())
    }

    if !no_authentication_confirmed && config.shared_secret.is_none() {
        return arg_error!(
            "shared-secret",
            "Shared secret is None, but no authentication is not confirmed"
        );
    }

    if let Some(s) = args
        .value_of("url-path-prefix")
        .map(std::string::ToString::to_string)
    {
        config.url_path_prefix = Some(s);
    };

    config.check()?;
    if args.is_present("print-config") {
        println!("{}", serde_yaml::to_string(&config).unwrap());
        std::process::exit(0);
    }

    Ok(config)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::init::init_default_config;
    #[test]
    fn test_basic_args() {
        init_default_config();
        let c = parse_args_from(&["audioserve", "--no-authentication", "test_data"]).unwrap();
        assert_eq!(1, c.base_dirs.len());

        let c = parse_args_from(&[
            "audioserve",
            "--listen",
            "127.0.0.1:4444",
            "--thread-pool-large",
            "--thread-pool-keep-alive-secs",
            "60",
            "--shared-secret",
            "usak",
            "--transcoding-max-parallel-processes",
            "99",
            "--transcoding-max-runtime",
            "99",
            "--token-validity-days",
            "99",
            "--client-dir",
            "test_data",
            "--secret-file",
            "test_data/some_secret",
            "--chapters-from-duration",
            "99",
            "--chapters-duration",
            "99",
            "--cors",
            "--url-path-prefix",
            "/user/audioserve",
            "test_data",
            "client",
        ])
        .unwrap();
        assert_eq!(2, c.base_dirs.len());
        assert_eq!("127.0.0.1:4444".parse::<SocketAddr>().unwrap(), c.listen);
        assert_eq!(16, c.thread_pool.num_threads);
        assert_eq!(1000, c.thread_pool.queue_size);
        assert_eq!(Some(Duration::from_secs(60)), c.thread_pool.keep_alive);
        assert_eq!(Some("usak".into()), c.shared_secret);
        assert_eq!(99, c.transcoding.max_parallel_processes);
        assert_eq!(99, c.transcoding.max_runtime_hours);
        assert_eq!(99 * 24, c.token_validity_hours);
        assert_eq!(PathBuf::from("test_data"), c.client_dir);
        assert_eq!(PathBuf::from("test_data/some_secret"), c.secret_file);
        assert_eq!(99, c.chapters.from_duration);
        assert_eq!(99, c.chapters.duration);
        assert!(c.cors);
        assert_eq!("/user/audioserve", c.url_path_prefix.unwrap())
    }

    #[test]
    #[cfg(feature = "transcoding-cache")]
    fn test_t_cache() {
        init_default_config();
        let c = parse_args_from(&[
            "audioserve",
            "--no-authentication",
            "--t-cache-dir",
            "test_data",
            "--t-cache-size",
            "999",
            "--t-cache-max-files",
            "999",
            "--t-cache-save-often",
            "test_data",
        ])
        .unwrap();

        assert_eq!(PathBuf::from("test_data"), c.transcoding.cache.root_dir);
        assert_eq!(999, c.transcoding.cache.max_size);
        assert_eq!(999, c.transcoding.cache.max_files);
        assert!(!c.transcoding.cache.disabled);
        assert!(c.transcoding.cache.save_often);
    }

    #[test]
    #[cfg(feature = "tls")]
    fn test_tls() {
        init_default_config();
        let c = parse_args_from(&[
            "audioserve",
            "--no-authentication",
            "--ssl-key",
            "test_data/desc.txt",
            "--ssl-key-password",
            "neco",
            "test_data",
        ])
        .unwrap();

        assert!(c.ssl.is_some());
        let ssl = c.ssl.unwrap();
        assert_eq!(PathBuf::from("test_data/desc.txt"), ssl.key_file);
        assert_eq!("neco", ssl.key_password);
    }

    #[test]
    #[cfg(feature = "symlinks")]
    fn test_symlinks_in_env() {
        init_default_config();
        env::set_var("AUDIOSERVE_ALLOW_SYMLINKS", "1");
        let c = parse_args_from(&["audioserve", "--no-authentication", "test_data"]).unwrap();

        assert!(c.allow_symlinks);
        env::remove_var("AUDIOSERVE_ALLOW_SYMLINKS");
    }

    #[test]
    fn test_from_config() {
        init_default_config();
        let c =
            parse_args_from(&["audioserve", "--config", "test_data/sample-config.yaml"]).unwrap();

        assert_eq!("neco", c.ssl.as_ref().unwrap().key_password);
        assert_eq!(Some("asecret".into()), c.shared_secret);
        assert_eq!(Some("/user/audioserve".into()), c.url_path_prefix);
    }
}
