use std::process::exit;

use super::validators::*;
use super::*;
use clap::{
    builder::FalseyValueParser, crate_authors, crate_name, value_parser, Arg, ArgAction, Command,
};
use collection::tags::{ALLOWED_TAGS, BASIC_TAGS};

fn create_parser() -> Command {
    let mut parser = Command::new(crate_name!())
        .version(LONG_VERSION)
        .author(crate_authors!())
        .arg(Arg::new("config")
            .short('g')
            .long("config")
            .num_args(1)
            .env("AUDIOSERVE_CONFIG")
            .value_parser(is_existing_file)
            .help("Configuration file in YAML format")
            )
        .arg(Arg::new("features")
            .long("features")
            .action(ArgAction::SetTrue)
            .help("Prints features, with which program is compiled and exits")
            )
        .arg(Arg::new("print-config")
            .long("print-config")
            .action(ArgAction::SetTrue)
            .help("Will print current config, with all other options to stdout, useful for creating config file")
            )
        .arg(Arg::new("data-dir")
            .long("data-dir")
            .num_args(1)
            .value_parser(parent_dir_exists)
            .env("AUDIOSERVE_DATA_DIR")
            .help("Base directory for data created by audioserve (caches, state, ...) [default is $HOME/.audioserve]")
            )
        .arg(Arg::new("debug")
            .short('d')
            .long("debug")
            .action(ArgAction::SetTrue)
            .help("Enable debug logging (detailed logging config can be done via RUST_LOG env. variable). Program must be compiled in debug configuration")
            )
        .arg(Arg::new("listen")
            .short('l')
            .long("listen")
            .help("Address and port server is listening on as address:port (by default listen on port 3000 on all interfaces)")
            .num_args(1)
            .value_parser(value_parser!(SocketAddr))
            .env("AUDIOSERVE_LISTEN")
            )
        .arg(Arg::new("thread-pool-large")
            .long("thread-pool-large")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_THREAD_POOL_LARGE")
            .help("Use larger thread pool (usually will not be needed)")       
            )
        .arg(Arg::new("thread-pool-keep-alive-secs")
            .long("thread-pool-keep-alive-secs")
            .num_args(1)
            .help("Threads in pool will shutdown after given seconds, if there is no work. Default is to keep threads forever.")
            .env("AUDIOSERVE_THREAD_POOL_KEEP_ALIVE")
            .value_parser(value_parser!(u64))
            )
        .arg(Arg::new("base-dir")
            .value_name("BASE_DIR")
            .num_args(1..=100)
            .env("AUDIOSERVE_BASE_DIRS")
            .value_delimiter(';')
            .help("Root directories for audio books, also referred as collections, you can also add :<options> after directory path to change collection behaviour, use --help-dir-options for more details")

            )
        .arg(Arg::new("help-dir-options")
            .long("help-dir-options")
            .action(ArgAction::SetTrue)
            .help("Prints help for collections directories options")
            )
        .arg(Arg::new("help-tags")
            .long("help-tags")
            .action(ArgAction::SetTrue)
            .help("Prints help for tags options")
        )
        .arg(Arg::new("tags")
            .long("tags")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_TAGS")
            .help("Collects prefered tags from audiofiles, use argument --help-tags for more details")
            .conflicts_with("tags-custom")
            )
        .arg(Arg::new("tags-custom")
            .long("tags-custom")
            .help("Collects custom tags from audiofiles, list tags searated by comma, use argument --help-tags for more details")
            .conflicts_with("tags")
            .num_args(1..100)
            .value_delimiter(',')
            .env("AUDIOSERVE_TAGS_CUSTOM")
            )
        .arg(Arg::new("no-authentication")
            .long("no-authentication")
            .action(ArgAction::SetTrue)
            .help("no authentication required - mainly for testing purposes")
            )
        .arg(Arg::new("shared-secret")
            .short('s')
            .long("shared-secret")
            .num_args(1)
            // .conflicts_with("no-authentication")
            // .required_unless_one(&["no-authentication", "shared-secret-file"])
            .env("AUDIOSERVE_SHARED_SECRET")
            .help("Shared secret for client authentication")
            )
        .arg(Arg::new("limit-rate")
            .long("limit-rate")
            .env("AUDIOSERVE_LIMIT_RATE")
            .num_args(1)
            .value_parser(value_parser!(f32))
            .help("Limits number of http request to x req/sec. Assures that resources are not exhausted in case of DDoS (but will also limit you). It's bit experimental now.")
            )
        .arg(Arg::new("shared-secret-file")
            .long("shared-secret-file")
            .num_args(1)
            // .conflicts_with("no-authentication")
            // .required_unless_one(&["no-authentication", "shared-secret"])
            .env("AUDIOSERVE_SHARED_SECRET_FILE")
            .help("File containing shared secret, it's slightly safer to read it from file, then provide as command argument")
            )
        .arg(Arg::new("transcoding-max-parallel-processes")
            .short('x')
            .long("transcoding-max-parallel-processes")
            .num_args(1)
            .value_parser(value_parser!(usize))
            .env("AUDIOSERVE_MAX_PARALLEL_PROCESSES")
            .help("Maximum number of concurrent transcoding processes, minimum is 4 [default: 2 * number of cores]")
            )
        .arg(Arg::new("transcoding-max-runtime")
            .long("transcoding-max-runtime")
            .num_args(1)
            .value_parser(value_parser!(u32))
            .env("AUDIOSERVE_TRANSCODING_MAX_RUNTIME")
            .help("Max duration of transcoding process in hours. If takes longer process is killed. [default is 24h]")

            )
        .arg(Arg::new("token-validity-days")
            .long("token-validity-days")
            .num_args(1)
            .value_parser(value_parser!(u32))
            .env("AUDIOSERVE_TOKEN_VALIDITY_DAYS")
            .help("Validity of authentication token issued by this server in days[default 365, min 10]")
            )
        .arg(Arg::new("client-dir")
            .short('c')
            .long("client-dir")
            .num_args(1)
            .env("AUDIOSERVE_CLIENT_DIR")
            .value_parser(is_existing_dir)
            .help("Directory with client files - index.html and bundle.js")

            )
        .arg(Arg::new("secret-file")
            .long("secret-file")
            .num_args(1)
            .value_parser(parent_dir_exists)
            .env("AUDIOSERVE_SECRET_FILE")
            .help("Path to file where server secret is kept - it's generated if it does not exists [default: is $HOME/.audioserve.secret]")
            )
        .arg(Arg::new("cors")
            .long("cors")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_CORS")
            .help("Enable CORS for all origins unless more specific origin is specified with --cors-regex")
            )
        .arg(Arg::new("cors-regex")
            .long("cors-regex")
            .help("Enable CORS only for origins that matches given regular expression")
            .env("AUDIOSERVE_CORS_REGEX")
            .requires("cors")
            .num_args(1)
            )
        .arg(Arg::new("chapters-from-duration")
            .long("chapters-from-duration")
            .num_args(1)
            .value_parser(value_parser!(u32))
            .env("AUDIOSERVE_CHAPTERS_FROM_DURATION")
            .help("forces split of audio file larger then x mins into chapters (not physically, but it'll be just visible as folder with chapters)[default:0 e.g. disabled]")
            )
        .arg(Arg::new("chapters-duration")
            .long("chapters-duration")
            .num_args(1)
            .value_parser(value_parser!(u32))
            .env("AUDIOSERVE_CHAPTERS_FROM_DURATION")
            .help("If long files is presented as chapters, one chapter has x mins [default: 30]")
            )
        .arg(Arg::new("no-dir-collaps")
            .long("no-dir-collaps")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_NO_DIR_COLLAPS")
            .help("Prevents automatic collaps/skip of directory with single chapterized audio file")
            )
        .arg(Arg::new("ignore-chapters-meta")
            .long("ignore-chapters-meta")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_IGNORE_CHAPTERS_META")
            .help("Ignore chapters metadata, so files with chapters will not be presented as folders")
            )
        .arg(Arg::new("url-path-prefix")
            .long("url-path-prefix")
            .num_args(1)
            .value_parser(is_valid_url_path_prefix)
            .env("AUDIOSERVE_URL_PATH_PREFIX")
            .help("Base URL is a fixed path that is before audioserve path part, must start with / and not end with /  [default: none]")
            )
        .arg(Arg::new("force-cache-update")
            .long("force-cache-update")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_FORCE_CACHE_UPDATE")
            .help("Forces full reload of metadata cache on start")
            )
        .arg(Arg::new("static-resource-cache-age")
            .long("static-resource-cache-age")
            .env("AUDIOSERVE_STATIC_RESOURCE_CACHE_AGE")
            .num_args(1)
            .help("Age for Cache-Control of static resources, 'no-store' or number of secs, 0 means Cache-Control is not sent [default no-store]")
        )
        .arg(Arg::new("folder-file-cache-age")
            .long("folder-file-cache-age")
            .env("AUDIOSERVE_FOLDER_FILE_CACHE_AGE")
            .num_args(1)
            .help("Age for Cache-Control of cover and text files in audio folders, 'no-store' or number of secs, 0 means Cache-Control is not sent [default 1 day]")
        )
        .arg(
            Arg::new("collapse-cd-folders")
            .long("collapse-cd-folders")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_COLLAPSE_CD_FOLDERS")
            .help("Collapses multi CD folders into one root folder, CD subfolders are recognized by regular expression")
        )
        .arg(
            Arg::new("cd-folder-regex")
            .long("cd-folder-regex")
            .num_args(1)
            .requires("collapse-cd-folders")
            .env("AUDIOSERVE_CD_FOLDER_REGEX")
            .help("Regular expression to recognize CD subfolder, if want to use other then default")
        )
        .arg(
            Arg::new("icons-cache-dir")
            .long("icons-cache-dir")
            .num_args(1)
            .env("AUDIOSERVE_ICONS_CACHE_DIR")
            .value_parser(parent_dir_exists)
            .help("Directory for icons cache [default is ~/.audioserve/icons-cache]")
        ).arg(
            Arg::new("icons-cache-size")
            .long("icons-cache-size")
            .num_args(1)
            .env("AUDIOSERVE_ICONS_CACHE_SIZE")
            .value_parser(value_parser!(u64))
            .help("Max size of icons cache in MBi, when reached LRU items are deleted, [default is 100]")
        ).arg(
            Arg::new("icons-cache-max-files")
            .long("icons-cache-max-files")
            .num_args(1)
            .env("AUDIOSERVE_ICONS_CACHE_MAX_FILES")
            .value_parser(value_parser!(u64))
            .help("Max number of files in icons cache, when reached LRU items are deleted, [default is 1024]")
        ).arg(
            Arg::new("icons-cache-disable")
            .long("icons-cache-disable")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_ICONS_CACHE_DISABLE")
            .conflicts_with_all(&["icons-cache-save-often", "icons-cache-max-files", "icons-cache-size", "icons-cache-dir"])
            .help("Icons cache is disabled.")
            )
        .arg(
            Arg::new("icons-cache-save-often")
            .long("icons-cache-save-often")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_ICONS_CACHE_SAVE_OFTEN")
            .help("Save additions to icons cache often, after each addition, this is normally not necessary")
        )
        .arg(
            Arg::new("icons-size")
            .long("icons-size")
            .num_args(1)
            .env("AUDIOSERVE_ICONS_SIZE")
            .value_parser(value_parser!(u64))
            .help("Size of folder icon in pixels, [default is 128]")
        )
        .arg(
            Arg::new("icons-fast-scaling")
            .long("icons-fast-scaling")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_ICONS_FAST_SCALING")
            .help("Use faster image scaling (linear triangle), by default slower, but better method (Lanczos3)")
        );

    if cfg!(feature = "behind-proxy") {
        parser = parser.arg(Arg::new("behind-proxy")
        .long("behind-proxy")
        .action(ArgAction::SetTrue)
        .value_parser(FalseyValueParser::new())
        .env("AUDIOSERVE_BEHIND_PROXY")
        .help("Informs program that it is behind remote proxy, now used only for logging (to get true remote client ip)")
        )
    }

    if cfg!(feature = "folder-download") {
        parser = parser.arg(
            Arg::new("disable-folder-download")
                .long("disable-folder-download")
                .action(ArgAction::SetTrue)
                .value_parser(FalseyValueParser::new())
                .env("AUDIOSERVE_DISABLE_FOLDER_DOWNLOAD")
                .help("Disables API point for downloading whole folder"),
        );
    }

    if cfg!(feature = "tls") {
        parser = parser.arg(Arg::new("ssl-key")
            .long("ssl-key")
            .num_args(1)
            .requires("ssl-cert")
            .value_parser(is_existing_file)
            .env("AUDIOSERVE_SSL_KEY")
            .help("TLS/SSL private key in PEM format, https is used")
            )
            .arg(Arg::new("ssl-cert")
            .long("ssl-cert")
            .requires("ssl-key")
            .value_parser(is_existing_file)
            .env("AUDIOSERVE_SSL_CERT")
            )
            .arg(Arg::new("ssl-key-password")
                .long("ssl-key-password")
                .num_args(1)
                .env("AUDIOSERVE_SSL_KEY_PASSWORD")
                .help("Deprecated - for PEM key password is not needed, so it should not be encrypted - default from rustls")
            );
    }

    if cfg!(feature = "shared-positions") {
        parser = parser.arg(
            Arg::new("positions-backup-file")
            .long("positions-backup-file")
            .num_args(1)
            .value_parser(parent_dir_exists)
            .env("AUDIOSERVE_POSITIONS_BACKUP_FILE")
            .help("File to back up last listened positions (can be used to restore positions as well, so has two slightly different uses) [default is None]"),
        )
        .arg(
            Arg::new("positions-ws-timeout")
            .long("positions-ws-timeout")
            .value_parser(value_parser!(u64))
            .env("AUDIOSERVE_POSITIONS_WS_TIMEOUT")
            .help("Timeout in seconds for idle websocket connection use for playback position sharing [default 600s]")
        )
        .arg(
            Arg::new("positions-restore")
            .long("positions-restore")
            .num_args(1)
            .value_parser(["legacy", "v1"])
            .env("AUDIOSERVE_POSITIONS_RESTORE")
            .requires("positions-backup-file")
            .help("Restores positions from backup JSON file, value is version of file legacy is before audioserve v0.16,  v1 is current")
        )
        .arg(
            Arg::new("positions-backup-schedule")
            .long("positions-backup-schedule")
            .num_args(1)
            .env("AUDIOSERVE_POSITIONS_BACKUP_SCHEDULE")
            .requires("positions-backup-file")
            .help("Sets regular schedule for backing up playback position - should be cron expression m h dom mon dow- minute (m), hour (h), day of month (dom), month (mon) day of week (dow)")
        );
    }

    if cfg!(feature = "symlinks") {
        parser = parser.arg(
            Arg::new("allow-symlinks")
                .long("allow-symlinks")
                .action(ArgAction::SetTrue)
                .value_parser(FalseyValueParser::new())
                .env("AUDIOSERVE_ALLOW_SYMLINKS")
                .help("Will follow symbolic/soft links in collections directories"),
        );
    }

    if cfg!(feature = "tags-encoding") {
        parser = parser.arg(
            Arg::new("tags-encoding")
                .num_args(1)
                .long("tags-encoding")
                .env("AUDIOSERVE_TAGS_ENCODING")
                .help(
                    "Alternate character encoding for audio tags metadata, if UTF8 decoding fails",
                ),
        )
    }

    parser = parser.arg(Arg::new("search-cache").long("search-cache").help(
        "Deprecated: does nothing. For caching config use :<options> on individual collections dirs params",
    ));

    if cfg!(feature = "transcoding-cache") {
        parser=parser.arg(
            Arg::new("t-cache-dir")
            .long("t-cache-dir")
            .num_args(1)
            .env("AUDIOSERVE_T_CACHE_DIR")
            .value_parser(parent_dir_exists)
            .help("Directory for transcoding cache [default is ~/.audioserve/audioserve-cache]")
        ).arg(
            Arg::new("t-cache-size")
            .long("t-cache-size")
            .num_args(1)
            .env("AUDIOSERVE_T_CACHE_SIZE")
            .value_parser(value_parser!(u64))
            .help("Max size of transcoding cache in MBi, when reached LRU items are deleted, [default is 1024]")
        ).arg(
            Arg::new("t-cache-max-files")
            .long("t-cache-max-files")
            .num_args(1)
            .env("AUDIOSERVE_T_CACHE_MAX_FILES")
            .value_parser(value_parser!(u32))
            .help("Max number of files in transcoding cache, when reached LRU items are deleted, [default is 1024]")
        ).arg(
            Arg::new("t-cache-disable")
            .long("t-cache-disable")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_T_CACHE_DISABLE")
            .conflicts_with_all(&["t-cache-save-often", "t-cache-max-files", "t-cache-size", "t-cache-dir"])
            .help("Transaction cache is disabled. If you want to completely get rid of it, compile without 'transcoding-cache'")
            )
        .arg(
            Arg::new("t-cache-save-often")
            .long("t-cache-save-often")
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_T_CACHE_SAVE_OFTEN")
            .help("Save additions to cache often, after each addition, this is normally not necessary")
        )
    }

    parser
}

macro_rules!  arg_error {
    ($arg:expr, $msg:expr) => {
        Error::in_argument_result($arg, $msg)
    };

    ($arg:expr, $msg:expr, $($param:expr),+) => {
        Error::in_argument_result($arg,
        format!($msg, $($param),+))
    };

}

macro_rules! has_flag {
    ($args:ident, $name:expr) => {
        $args.remove_one($name).unwrap_or_default()
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
    let mut args = p.get_matches_from(args);

    if has_flag!(args, "help-dir-options") {
        print_dir_options_help();
        exit(0);
    }

    if has_flag!(args, "help-tags") {
        print_tags_help();
        exit(0);
    }

    if has_flag!(args, "features") {
        println!("{}", FEATURES);
        exit(0);
    }

    if let Some(dir) = args.get_one::<PathBuf>("data-dir") {
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

    let mut config: Config = if let Some(config_file) = args.get_one::<PathBuf>("config") {
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

    // let is_present_or_env = |name: &str, env_name: &str| {
    //     has_flag!(args,name) || env::var(env_name).map(|s| !s.is_empty()).unwrap_or(false)
    // };

    if has_flag!(args, "debug") {
        let name = "RUST_LOG";
        if env::var_os(name).is_none() {
            env::set_var(name, "debug");
        }
    }

    if let Some(base_dirs) = args.get_many::<String>("base-dir") {
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
    } else if let Some(addr) = args.remove_one::<SocketAddr>("listen") {
        config.listen = addr;
    }

    if has_flag!(args, "thread-pool-large") {
        config.thread_pool = ThreadPoolConfig {
            num_threads: 16,
            queue_size: 1000,
            keep_alive: None,
        };
    }

    if has_flag!(args, "no-authentication") {
        config.shared_secret = None;
        no_authentication_confirmed = true
    } else if let Some(secret) = args.remove_one("shared-secret") {
        config.shared_secret = Some(secret)
    } else if let Some(file) = args.remove_one::<PathBuf>("shared-secret-file") {
        config.set_shared_secret_from_file(file)?
    };

    if let Some(r) = args.remove_one("limit-rate") {
        config.limit_rate = Some(r)
    }

    if let Some(n) = args.remove_one("transcoding-max-parallel-processes") {
        config.transcoding.max_parallel_processes = n;
    }

    if let Some(n) = args.remove_one("transcoding-max-runtime") {
        config.transcoding.max_runtime_hours = n;
    }

    if let Some(v) = args.remove_one("thread-pool-keep-alive-secs") {
        config.thread_pool.keep_alive = Some(Duration::from_secs(v))
    }

    if let Some(validity) = args.remove_one::<u32>("token-validity-days") {
        config.token_validity_hours = validity * 24
    }

    if let Some(client_dir) = args.remove_one("client-dir") {
        config.client_dir = client_dir;
    }
    if let Some(secret_file) = args.remove_one("secret-file") {
        config.secret_file = secret_file;
    }

    if has_flag!(args, "cors") {
        config.cors = match args.remove_one("cors-regex") {
            Some(o) => Some(CorsConfig {
                inner: Cors::default(),
                regex: Some(o),
            }),
            None => Some(CorsConfig::default()),
        }
    }

    if has_flag!(args, "collapse-cd-folders") {
        config.collapse_cd_folders = match args.remove_one("cd-folder-regex") {
            Some(re) => Some(CollapseCDFolderConfig { regex: Some(re) }),
            None => Some(CollapseCDFolderConfig::default()),
        }
    }

    if has_flag!(args, "force-cache-update") {
        config.force_cache_update_on_init = true
    }

    if let Some(tags) = args.remove_many("tags-custom") {
        for t in tags {
            if !ALLOWED_TAGS.contains(&t) {
                arg_error!("tags-custom", "Unknown tag")?
            }
            config.tags.insert(t.to_string());
        }
    } else if has_flag!(args, "tags") {
        config.tags.extend(BASIC_TAGS.iter().map(|i| i.to_string()));
    }

    let parse_cache_age = |age: &str| {
        if age == "no-store" {
            Ok(None)
        } else {
            age.parse::<u32>()
                .or_else(|_| arg_error!("*-resource-cache-age", "Invalid value"))
                .map(Some)
        }
    };

    if let Some(age) = args.remove_one("static-resource-cache-age") {
        config.static_resource_cache_age = parse_cache_age(age)?;
    }

    if let Some(age) = args.remove_one("folder-file-cache-age") {
        config.folder_file_cache_age = parse_cache_age(age)?;
    }

    if let Some(n) = args.remove_one("icons-size") {
        config.icons.size = n;
    }

    if let Some(d) = args.remove_one("icons-cache-dir") {
        config.icons.cache_dir = d;
    }

    if let Some(n) = args.remove_one("icons-cache-size") {
        config.icons.cache_max_size = n;
    }

    if has_flag!(args, "icons-cache-disable") {
        config.icons.cache_disabled = true;
    }

    if has_flag!(args, "icons-fast-scaling") {
        config.icons.fast_scaling = true;
    }

    if has_flag!(args, "icons-cache-save-often") {
        config.icons.cache_save_often = true;
    }

    if cfg!(feature = "symlinks") && has_flag!(args, "allow-symlinks") {
        config.allow_symlinks = true
    }

    #[cfg(feature = "tls")]
    {
        if let Some(key) = args.remove_one("ssl-key") {
            let key_file = key;
            let cert_file = args.remove_one("ssl-cert").unwrap();
            config.ssl = Some(SslConfig {
                key_file,
                cert_file,
                key_password: "".into(),
            });
        }
    }

    #[cfg(feature = "transcoding-cache")]
    {
        if let Some(d) = args.remove_one("t-cache-dir") {
            config.transcoding.cache.root_dir = d;
        }

        if let Some(n) = args.remove_one("t-cache-size") {
            config.transcoding.cache.max_size = n;
        }

        if let Some(n) = args.remove_one("t-cache-max-files") {
            config.transcoding.cache.max_files = n;
        }

        if has_flag!(args, "t-cache-disable") {
            config.transcoding.cache.disabled = true;
        }

        if has_flag!(args, "t-cache-save-often") {
            config.transcoding.cache.save_often = true;
        }
    };
    if cfg!(feature = "folder-download") {
        if has_flag!(args, "disable-folder-download") {
            config.disable_folder_download = true
        }
    } else {
        config.disable_folder_download = true
    };

    if has_flag!(args, "behind-proxy") {
        config.behind_proxy = true;
    }

    if let Some(d) = args.remove_one("chapters-from-duration") {
        config.chapters.from_duration = d;
    }

    if let Some(d) = args.remove_one("chapters-duration") {
        config.chapters.duration = d;
    }

    if has_flag!(args, "no-dir-collaps") {
        config.no_dir_collaps = true;
    }

    if has_flag!(args, "ignore-chapters-meta") {
        config.ignore_chapters_meta = true;
    }

    #[cfg(feature = "shared-positions")]
    {
        if let Some(ps) = args.remove_one("positions-restore") {
            config.positions.restore = ps;
            no_authentication_confirmed = true;
        }

        if let Some(positions_backup_file) = args.remove_one("positions-backup-file") {
            config.positions.backup_file = Some(positions_backup_file);
        }

        if let Some(positions_ws_timeout) = args.remove_one("positions-ws-timeout") {
            config.positions.ws_timeout = Duration::from_secs(positions_ws_timeout)
        }

        if let Some(positions_backup_schedule) = args.remove_one("positions-backup-schedule") {
            config.positions.backup_schedule = Some(positions_backup_schedule);
        }
    }

    #[cfg(feature = "tags-encoding")]
    {
        if let Some(enc) = args.remove_one("tags-encoding") {
            config.tags_encoding = Some(enc)
        }
    }

    if !no_authentication_confirmed && config.shared_secret.is_none() {
        return arg_error!(
            "shared-secret",
            "Shared secret is None, but no authentication is not confirmed"
        );
    }

    if let Some(s) = args.remove_one("url-path-prefix") {
        config.url_path_prefix = Some(s);
    };

    config.check()?;
    config.prepare()?;
    if has_flag!(args, "print-config") {
        println!("{}", serde_yaml::to_string(&config).unwrap());
        exit(0);
    }

    Ok(config)
}

fn print_dir_options_help() {
    print!(
        "
Options can be used to change behavior of particular collection directory and override 
some global arguments.
Option is added after collection path argument separated with : and individual options
are separated by , . 
Options can have values - after =. For boolean options, value is optional and default is true.
Examples: 
/my/audio:no-cache
/other/audio:ignore-chapters-meta=false,allow-symlinks,no-dir-collaps=true,tags=title+album+artist

Available options:
nc or no-cache          <=true|false> directory will not use cache (browsing and search will be 
                        slower for large collection, playback position sharing and metadata tags 
                        will not work)
force-cache-update      <=true|false> always do full cache update on start
ignore-chapters-meta    <=true|false> ignore chapters metadata in audio files. Instead present as
                        one big audio file
allow-symlinks          <=true|false>  follow symbolic links
no-dir-collaps          <=true|false> do not collaps directories with single chapterized audio file
chapters-duration       =x  duration (mins) of chapter for cutting of large audio files
chapters-from-duration  =x  min.duration (mins) of large audio file to be cut to chapters
tags                    =tag1+tag2...  metadata tags to collect (supported tags names separated by +)
default-tags            <=true|false>  collect default tags. Use --help-tags argument to get more 
                        information about supported metadata tags 


"
    )
}

pub fn print_tags_help() {
    print!("
You can define metadata tags, that will be collected from audiofiles and presented via API with folder information.
Tags that will be same for all audiofiles in folder will be available on folder level, tags that differs per file
will be present on file level. 
You need to opt in for tags to be included, either use --tags argument to include preferred preselected tags or --tags-custom,
where you can select tags you want separated by comma.
BE AWARE: if you use or change tags arguments all collection cache has to be rescaned! 

Preferred tags are: 
");
    print_tags(BASIC_TAGS);

    println!("\nAvailable tags are:");

    print_tags(ALLOWED_TAGS);
}

fn print_tags(list: &[&str]) {
    list.chunks(8).for_each(|c| {
        let row = c.to_vec().join(", ");
        println!("{},", row)
    })
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
            "--cors-regex",
            "mameluci",
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
        assert!(matches!(
            c.cors.unwrap().inner,
            Cors::AllowMatchingOrigins(_)
        ));
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
            "--ssl-cert",
            "test_data/desc.txt",
            "test_data",
        ])
        .unwrap();

        assert!(c.ssl.is_some());
        let ssl = c.ssl.unwrap();
        assert_eq!(PathBuf::from("test_data/desc.txt"), ssl.key_file);
        assert_eq!(PathBuf::from("test_data/desc.txt"), ssl.cert_file);
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

        assert_eq!(
            Path::new("test_data/desc.txt"),
            c.ssl.as_ref().unwrap().key_file
        );
        assert_eq!(Some("asecret".into()), c.shared_secret);
        assert_eq!(Some("/user/audioserve".into()), c.url_path_prefix);
        assert!(matches!(c.cors.unwrap().inner, Cors::AllowAllOrigins));
    }
}
