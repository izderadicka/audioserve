use std::process::exit;

use collection::tags::{ALLOWED_TAGS, BASIC_TAGS};

use super::validators::*;
use super::*;
use clap::{crate_authors, crate_name, App, Arg};

type Parser<'a> = App<'a, 'a>;

fn create_parser<'a>() -> Parser<'a> {
    let mut parser = App::new(crate_name!())
        .version(LONG_VERSION)
        .author(crate_authors!())
        .arg(Arg::with_name("config")
            .short("g")
            .long("config")
            .takes_value(true)
            .env("AUDIOSERVE_CONFIG")
            .validator_os(is_existing_file)
            .help("Configuration file in YAML format")
            )
        .arg(Arg::with_name("features")
            .long("features")
            .help("Prints features, with which program is compiled and exits")
            )
        .arg(Arg::with_name("print-config")
            .long("print-config")
            .help("Will print current config, with all other options to stdout, useful for creating config file")
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
            .value_delimiter(";")
            .help("Root directories for audio books, also referred as collections, you can also add :<options> after directory path to change collection behaviour, use --help-dir-options for more details")

            )
        .arg(Arg::with_name("help-dir-options")
            .long("help-dir-options")
            .help("Prints help for collections directories options")
            )
        .arg(Arg::with_name("help-tags")
            .long("help-tags")
            .help("Prints help for tags options")
        )
        .arg(Arg::with_name("tags")
            .long("tags")
            .help("Collects prefered tags from audiofiles, use argument --help-tags for more details")
            .conflicts_with("tags-custom")
            )
        .arg(Arg::with_name("tags-custom")
            .long("tags-custom")
            .help("Collects custom tags from audiofiles, list tags searated by comma, use argument --help-tags for more details")
            .conflicts_with("tags")
            .takes_value(true)
            .multiple(true)
            .use_delimiter(true)
            .env("AUDIOSERVE_TAGS_CUSTOM")
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
            .help("Enable CORS for all origins unless more specific origin is specified with --cors-regex")
            )
        .arg(Arg::with_name("cors-regex")
            .long("cors-regex")
            .help("Enable CORS only for origins that matches given regular expression")
            .env("AUDIOSERVE_CORS_REGEX")
            .requires("cors")
            .takes_value(true)
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
            )
        .arg(Arg::with_name("force-cache-update")
            .long("force-cache-update")
            .help("Forces full reload of metadata cache on start")
            )
        .arg(Arg::with_name("static-resource-cache-age")
            .long("static-resource-cache-age")
            .env("AUDIOSERVE_STATIC_RESOURCE_CACHE_AGE")
            .takes_value(true)
            .help("Age for Cache-Control of static resources, 'no-store' or number of secs, 0 means Cache-Control is not sent [default no-store]")
        )
        .arg(Arg::with_name("folder-file-cache-age")
            .long("folder-file-cache-age")
            .env("AUDIOSERVE_FOLDER_FILE_CACHE_AGE")
            .takes_value(true)
            .help("Age for Cache-Control of cover and text files in audio folders, 'no-store' or number of secs, 0 means Cache-Control is not sent [default 1 day]")
        )
        .arg(
            Arg::with_name("collapse-cd-folders")
            .long("collapse-cd-folders")
            .help("Collapses multi CD folders into one root folder, CD subfolders are recognized by regular expression")
        )
        .arg(
            Arg::with_name("cd-folder-regex")
            .long("cd-folder-regex")
            .takes_value(true)
            .requires("collapse-cd-folders")
            .env("AUDIOSERVE_CD_FOLDER_REGEX")
            .help("Regular expression to recognize CD subfolder, if want to use other then default")
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
            .requires("ssl-cert")
            .validator_os(is_existing_file)
            .env("AUDIOSERVE_SSL_KEY")
            .help("TLS/SSL private key in PEM format, https is used")
            )
            .arg(Arg::with_name("ssl-cert")
            .long("ssl-cert")
            .requires("ssl-key")
            .validator_os(is_existing_file)
            .env("AUDIOSERVE_SSL_CERT")
            )
            .arg(Arg::with_name("ssl-key-password")
                .long("ssl-key-password")
                .takes_value(true)
                .env("AUDIOSERVE_SSL_KEY_PASSWORD")
                .help("Deprecated - for PEM key password is not needed, so it should not be encrypted - default from rustls")
            );
    }

    if cfg!(feature = "shared-positions") {
        parser = parser.arg(
            Arg::with_name("positions-backup-file")
            .long("positions-backup-file")
            .takes_value(true)
            .validator_os(parent_dir_exists)
            .env("AUDIOSERVE_POSITIONS_BACKUP_FILE")
            .help("File to back up last listened positions (can be used to restore positions as well, so has two slightly different uses) [default is None]"),
        )
        .arg(
            Arg::with_name("positions-ws-timeout")
            .long("positions-ws-timeout")
            .validator(is_number)
            .env("AUDIOSERVE_POSITIONS_WS_TIMEOUT")
            .help("Timeout in seconds for idle websocket connection use for playback position sharing [default 600s]")
        )
        .arg(
            Arg::with_name("positions-restore")
            .long("positions-restore")
            .takes_value(true)
            .possible_values(&["legacy", "v1"])
            .env("AUDIOSERVE_POSITIONS_RESTORE")
            .requires("positions-backup-file")
            .help("Restores positions from backup JSON file, value is version of file legacy is before audioserve v0.16,  v1 is current")
        )
        .arg(
            Arg::with_name("positions-backup-schedule")
            .long("positions-backup-schedule")
            .takes_value(true)
            .env("AUDIOSERVE_POSITIONS_BACKUP_SCHEDULE")
            .requires("positions-backup-file")
            .help("Sets regular schedule for backing up playback position - should be cron expression m h dom mon dow- minute (m), hour (h), day of month (dom), month (mon) day of week (dow)")
        );
    }

    if cfg!(feature = "symlinks") {
        parser = parser.arg(
            Arg::with_name("allow-symlinks")
                .long("allow-symlinks")
                .help("Will follow symbolic/soft links in collections directories"),
        );
    }

    if cfg!(feature = "tags-encoding") {
        parser = parser.arg(
            Arg::with_name("tags-encoding")
                .takes_value(true)
                .long("tags-encoding")
                .env("AUDIOSERVE_TAGS_ENCODING")
                .help(
                    "Alternate character encoding for audio tags metadata, if UTF8 decoding fails",
                ),
        )
    }

    parser = parser.arg(Arg::with_name("search-cache").long("search-cache").help(
        "Deprecated: does nothing. For caching config use :<options> on individual collections dirs params",
    ));

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
        Error::in_argument_result($arg, $msg)
    };

    ($arg:expr, $msg:expr, $($param:expr),+) => {
        Error::in_argument_result($arg,
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

    if args.is_present("help-dir-options") {
        print_dir_options_help();
        exit(0);
    }

    if args.is_present("help-tags") {
        print_tags_help();
        exit(0);
    }

    if args.is_present("features") {
        println!("{}", FEATURES);
        exit(0);
    }

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

    if let Some(base_dirs) = args.values_of("base-dir") {
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
        config.cors = match args.value_of("cors-regex") {
            Some(o) => Some(CorsConfig {
                inner: Cors::default(),
                regex: Some(o.to_string()),
            }),
            None => Some(CorsConfig::default()),
        }
    }

    if is_present_or_env("collapse-cd-folders", "AUDIOSERVE_COLLAPSE_CD_FOLDERS") {
        config.collapse_cd_folders = match args.value_of("cd-folder-regex") {
            Some(re) => Some(CollapseCDFolderConfig {
                regex: Some(re.into()),
            }),
            None => Some(CollapseCDFolderConfig::default()),
        }
    }

    if is_present_or_env("force-cache-update", "AUDIOSERVE_FORCE_CACHE_UPDATE") {
        config.force_cache_update_on_init = true
    }

    if let Some(tags) = args.values_of("tags-custom") {
        for t in tags {
            if !ALLOWED_TAGS.contains(&t) {
                arg_error!("tags-custom", "Unknown tag")?
            }
            config.tags.insert(t.to_string());
        }
    } else if is_present_or_env("tags", "AUDIOSERVE_TAGS") {
        config.tags.extend(BASIC_TAGS.iter().map(|i| i.to_string()));
    }

    let parse_cache_age = |age: &str| {
        if age == "no-store" {
            Ok(None)
        } else {
            age.parse::<u32>()
                .or_else(|_| arg_error!("*-resource-cache-age", "Invalid value"))
                .map(|n| Some(n))
        }
    };

    if let Some(age) = args.value_of("static-resource-cache-age") {
        config.static_resource_cache_age = parse_cache_age(age)?;
    }

    if let Some(age) = args.value_of("folder-file-cache-age") {
        config.folder_file_cache_age = parse_cache_age(age)?;
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
            let cert_file = args.value_of("ssl-cert").unwrap().into();
            config.ssl = Some(SslConfig {
                key_file,
                cert_file,
                key_password: "".into(),
            });
        }
    }

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
    if cfg!(feature = "folder-download") {
        if is_present_or_env(
            "disable-folder-download",
            "AUDIOSERVE_DISABLE_FOLDER_DOWNLOAD",
        ) {
            config.disable_folder_download = true
        }
    } else {
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

    #[cfg(feature = "shared-positions")]
    {
        if let Some(ps) = args.value_of("positions-restore") {
            config.positions.restore = ps.parse().expect("Value was checked by clap");
            no_authentication_confirmed = true;
        }

        if let Some(positions_backup_file) = args.value_of_os("positions-backup-file") {
            config.positions.backup_file = Some(positions_backup_file.into());
        }

        if let Some(positions_ws_timeout) = args.value_of("positions-ws-timeout") {
            config.positions.ws_timeout = Duration::from_secs(positions_ws_timeout.parse().unwrap())
        }

        if let Some(positions_backup_schedule) = args.value_of("positions-backup-schedule") {
            config.positions.backup_schedule = Some(positions_backup_schedule.into());
        }
    }

    #[cfg(feature = "tags-encoding")]
    {
        if let Some(enc) = args.value_of("tags-encoding") {
            config.tags_encoding = Some(enc.into())
        }
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
    config.prepare()?;
    if args.is_present("print-config") {
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
        let row = c.iter().copied().collect::<Vec<_>>().join(", ");
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
