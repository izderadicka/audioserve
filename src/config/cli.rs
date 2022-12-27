use std::{env, fs::File, net::SocketAddr, path::PathBuf, process::exit};

use super::{
    base_data_dir, validators::*, CollapseCDFolderConfig, Config, Cors, CorsConfig, Error, Result,
    SslConfig, ThreadPoolConfig, BASE_DATA_DIR, FEATURES, LONG_VERSION,
};
use clap::{
    builder::FalseyValueParser, crate_authors, crate_name, value_parser, Arg, ArgAction, Command,
};
use collection::tags::{ALLOWED_TAGS, BASIC_TAGS};

const ARG_CONFIG: &str = "config";
const ARG_FEATURES: &str = "features";
const ARG_PRINT_CONFIG: &str = "print-config";
const ARG_DATA_DIR: &str = "data-dir";
const ARG_DEBUG: &str = "debug";
const ARG_LISTEN: &str = "listen";
const ARG_THREAD_POOL_LARGE: &str = "thread-pool-large";
const ARG_THREAD_POOL_KEEP_ALIVE_SECS: &str = "thread-pool-keep-alive-secs";
const ARG_BASE_DIR: &str = "base-dir";
const ARG_HELP_DIR_OPTIONS: &str = "help-dir-options";
const ARG_HELP_TAGS: &str = "help-tags";
const ARG_TAGS: &str = "tags";
const ARG_TAGS_CUSTOM: &str = "tags-custom";
const ARG_NO_AUTHENTICATION: &str = "no-authentication";
const ARG_SHARED_SECRET: &str = "shared-secret";
const ARG_LIMIT_RATE: &str = "limit-rate";
const ARG_SHARED_SECRET_FILE: &str = "shared-secret-file";
const ARG_TRANSCODING_MAX_PARALLEL_PROCESSES: &str = "transcoding-max-parallel-processes";
const ARG_TRANSCODING_MAX_RUNTIME: &str = "transcoding-max-runtime";
const ARG_TOKEN_VALIDITY_DAYS: &str = "token-validity-days";
const ARG_CLIENT_DIR: &str = "client-dir";
const ARG_SECRET_FILE: &str = "secret-file";
const ARG_CORS: &str = "cors";
const ARG_CORS_REGEX: &str = "cors-regex";
const ARG_CHAPTERS_FROM_DURATION: &str = "chapters-from-duration";
const ARG_CHAPTERS_DURATION: &str = "chapters-duration";
const ARG_NO_DIR_COLLAPS: &str = "no-dir-collaps";
const ARG_IGNORE_CHAPTERS_META: &str = "ignore-chapters-meta";
const ARG_URL_PATH_PREFIX: &str = "url-path-prefix";
const ARG_FORCE_CACHE_UPDATE: &str = "force-cache-update";
const ARG_STATIC_RESOURCE_CACHE_AGE: &str = "static-resource-cache-age";
const ARG_FOLDER_FILE_CACHE_AGE: &str = "folder-file-cache-age";
const ARG_COLLAPSE_CD_FOLDERS: &str = "collapse-cd-folders";
const ARG_CD_FOLDER_REGEX: &str = "cd-folder-regex";
const ARG_ICONS_CACHE_DIR: &str = "icons-cache-dir";
const ARG_ICONS_CACHE_SIZE: &str = "icons-cache-size";
const ARG_ICONS_CACHE_MAX_FILES: &str = "icons-cache-max-files";
const ARG_ICONS_CACHE_DISABLE: &str = "icons-cache-disable";
const ARG_ICONS_CACHE_SAVE_OFTEN: &str = "icons-cache-save-often";
const ARG_ICONS_SIZE: &str = "icons-size";
const ARG_ICONS_FAST_SCALING: &str = "icons-fast-scaling";
const ARG_BEHIND_PROXY: &str = "behind-proxy";
const ARG_DISABLE_FOLDER_DOWNLOAD: &str = "disable-folder-download";
const ARG_SSL_KEY: &str = "ssl-key";
const ARG_SSL_CERT: &str = "ssl-cert";
const ARG_SSL_KEY_PASSWORD: &str = "ssl-key-password";
const ARG_POSITIONS_BACKUP_FILE: &str = "positions-backup-file";
const ARG_POSITIONS_WS_TIMEOUT: &str = "positions-ws-timeout";
const ARG_POSITIONS_RESTORE: &str = "positions-restore";
const ARG_POSITIONS_BACKUP_SCHEDULE: &str = "positions-backup-schedule";
const ARG_ALLOW_SYMLINKS: &str = "allow-symlinks";
const ARG_TAGS_ENCODING: &str = "tags-encoding";
const ARG_SEARCH_CACHE: &str = "search-cache";
const ARG_T_CACHE_DIR: &str = "t-cache-dir";
const ARG_T_CACHE_SIZE: &str = "t-cache-size";
const ARG_T_CACHE_MAX_FILES: &str = "t-cache-max-files";
const ARG_T_CACHE_DISABLE: &str = "t-cache-disable";
const ARG_T_CACHE_SAVE_OFTEN: &str = "t-cache-save-often";

fn create_parser() -> Command {
    let mut parser = Command::new(crate_name!())
        .version(LONG_VERSION)
        .author(crate_authors!())
        .arg(Arg::new(ARG_CONFIG)
            .short('g')
            .long(ARG_CONFIG)
            .num_args(1)
            .env("AUDIOSERVE_CONFIG")
            .value_parser(is_existing_file)
            .help("Configuration file in YAML format")
            )
        .arg(Arg::new(ARG_FEATURES)
            .long(ARG_FEATURES)
            .action(ArgAction::SetTrue)
            .help("Prints features, with which program is compiled and exits")
            )
        .arg(Arg::new(ARG_PRINT_CONFIG)
            .long(ARG_PRINT_CONFIG)
            .action(ArgAction::SetTrue)
            .help("Will print current config, with all other options to stdout, useful for creating config file")
            )
        .arg(Arg::new(ARG_DATA_DIR)
            .long(ARG_DATA_DIR)
            .num_args(1)
            .value_parser(parent_dir_exists)
            .env("AUDIOSERVE_DATA_DIR")
            .help("Base directory for data created by audioserve (caches, state, ...) [default is $HOME/.audioserve]")
            )
        .arg(Arg::new(ARG_DEBUG)
            .short('d')
            .long(ARG_DEBUG)
            .action(ArgAction::SetTrue)
            .help("Enable debug logging (detailed logging config can be done via RUST_LOG env. variable). Program must be compiled in debug configuration")
            )
        .arg(Arg::new(ARG_LISTEN)
            .short('l')
            .long(ARG_LISTEN)
            .help("Address and port server is listening on as address:port (by default listen on port 3000 on all interfaces)")
            .num_args(1)
            .value_parser(value_parser!(SocketAddr))
            .env("AUDIOSERVE_LISTEN")
            )
        .arg(Arg::new(ARG_THREAD_POOL_LARGE)
            .long(ARG_THREAD_POOL_LARGE)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_THREAD_POOL_LARGE")
            .help("Use larger thread pool (usually will not be needed)")       
            )
        .arg(Arg::new(ARG_THREAD_POOL_KEEP_ALIVE_SECS)
            .long(ARG_THREAD_POOL_KEEP_ALIVE_SECS)
            .num_args(1)
            .help("Threads in pool will shutdown after given seconds, if there is no work. Default is to keep threads forever.")
            .env("AUDIOSERVE_THREAD_POOL_KEEP_ALIVE")
            .value_parser(duration_secs)
            )
        .arg(Arg::new(ARG_BASE_DIR)
            .value_name("BASE_DIR")
            .num_args(1..=100)
            .env("AUDIOSERVE_BASE_DIRS")
            .value_delimiter(';')
            .help("Root directories for audio books, also referred as collections, you can also add :<options> after directory path to change collection behaviour, use --help-dir-options for more details")

            )
        .arg(Arg::new(ARG_HELP_DIR_OPTIONS)
            .long(ARG_HELP_DIR_OPTIONS)
            .action(ArgAction::SetTrue)
            .help("Prints help for collections directories options")
            )
        .arg(Arg::new(ARG_HELP_TAGS)
            .long(ARG_HELP_TAGS)
            .action(ArgAction::SetTrue)
            .help("Prints help for tags options")
        )
        .arg(Arg::new(ARG_TAGS)
            .long(ARG_TAGS)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_TAGS")
            .help("Collects prefered tags from audiofiles, use argument --help-tags for more details")
            .conflicts_with(ARG_TAGS_CUSTOM)
            )
        .arg(Arg::new(ARG_TAGS_CUSTOM)
            .long(ARG_TAGS_CUSTOM)
            .help("Collects custom tags from audiofiles, list tags searated by comma, use argument --help-tags for more details")
            .conflicts_with(ARG_TAGS)
            .num_args(1..100)
            .value_delimiter(',')
            .env("AUDIOSERVE_TAGS_CUSTOM")
            )
        .arg(Arg::new(ARG_NO_AUTHENTICATION)
            .long(ARG_NO_AUTHENTICATION)
            .action(ArgAction::SetTrue)
            .help("no authentication required - mainly for testing purposes")
            )
        .arg(Arg::new(ARG_SHARED_SECRET)
            .short('s')
            .long(ARG_SHARED_SECRET)
            .num_args(1)
            // .conflicts_with(ARG_NO_AUTHENTICATION)
            // .required_unless_one(&[ARG_NO_AUTHENTICATION, ARG_SHARED_SECRET_FILE])
            .env("AUDIOSERVE_SHARED_SECRET")
            .help("Shared secret for client authentication")
            )
        .arg(Arg::new(ARG_LIMIT_RATE)
            .long(ARG_LIMIT_RATE)
            .env("AUDIOSERVE_LIMIT_RATE")
            .num_args(1)
            .value_parser(value_parser!(f32))
            .help("Limits number of http request to x req/sec. Assures that resources are not exhausted in case of DDoS (but will also limit you). It's bit experimental now.")
            )
        .arg(Arg::new(ARG_SHARED_SECRET_FILE)
            .long(ARG_SHARED_SECRET_FILE)
            .num_args(1)
            // .conflicts_with(ARG_NO_AUTHENTICATION)
            // .required_unless_one(&[ARG_NO_AUTHENTICATION, ARG_SHARED_SECRET])
            .env("AUDIOSERVE_SHARED_SECRET_FILE")
            .value_parser(is_existing_file)
            .help("File containing shared secret, it's slightly safer to read it from file, then provide as command argument")
            )
        .arg(Arg::new(ARG_TRANSCODING_MAX_PARALLEL_PROCESSES)
            .short('x')
            .long(ARG_TRANSCODING_MAX_PARALLEL_PROCESSES)
            .num_args(1)
            .value_parser(value_parser!(usize))
            .env("AUDIOSERVE_MAX_PARALLEL_PROCESSES")
            .help("Maximum number of concurrent transcoding processes, minimum is 4 [default: 2 * number of cores]")
            )
        .arg(Arg::new(ARG_TRANSCODING_MAX_RUNTIME)
            .long(ARG_TRANSCODING_MAX_RUNTIME)
            .num_args(1)
            .value_parser(value_parser!(u32))
            .env("AUDIOSERVE_TRANSCODING_MAX_RUNTIME")
            .help("Max duration of transcoding process in hours. If takes longer process is killed. [default is 24h]")

            )
        .arg(Arg::new(ARG_TOKEN_VALIDITY_DAYS)
            .long(ARG_TOKEN_VALIDITY_DAYS)
            .num_args(1)
            .value_parser(value_parser!(u32))
            .env("AUDIOSERVE_TOKEN_VALIDITY_DAYS")
            .help("Validity of authentication token issued by this server in days[default 365, min 10]")
            )
        .arg(Arg::new(ARG_CLIENT_DIR)
            .short('c')
            .long(ARG_CLIENT_DIR)
            .num_args(1)
            .env("AUDIOSERVE_CLIENT_DIR")
            .value_parser(is_existing_dir)
            .help("Directory with client files - index.html and bundle.js")

            )
        .arg(Arg::new(ARG_SECRET_FILE)
            .long(ARG_SECRET_FILE)
            .num_args(1)
            .value_parser(parent_dir_exists)
            .env("AUDIOSERVE_SECRET_FILE")
            .help("Path to file where server secret is kept - it's generated if it does not exists [default: is $HOME/.audioserve.secret]")
            )
        .arg(Arg::new(ARG_CORS)
            .long(ARG_CORS)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_CORS")
            .help("Enable CORS for all origins unless more specific origin is specified with --cors-regex")
            )
        .arg(Arg::new(ARG_CORS_REGEX)
            .long(ARG_CORS_REGEX)
            .help("Enable CORS only for origins that matches given regular expression")
            .env("AUDIOSERVE_CORS_REGEX")
            .requires(ARG_CORS)
            .num_args(1)
            )
        .arg(Arg::new(ARG_CHAPTERS_FROM_DURATION)
            .long(ARG_CHAPTERS_FROM_DURATION)
            .num_args(1)
            .value_parser(value_parser!(u32))
            .env("AUDIOSERVE_CHAPTERS_FROM_DURATION")
            .help("forces split of audio file larger then x mins into chapters (not physically, but it'll be just visible as folder with chapters)[default:0 e.g. disabled]")
            )
        .arg(Arg::new(ARG_CHAPTERS_DURATION)
            .long(ARG_CHAPTERS_DURATION)
            .num_args(1)
            .value_parser(value_parser!(u32))
            .env("AUDIOSERVE_CHAPTERS_FROM_DURATION")
            .help("If long files is presented as chapters, one chapter has x mins [default: 30]")
            )
        .arg(Arg::new(ARG_NO_DIR_COLLAPS)
            .long(ARG_NO_DIR_COLLAPS)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_NO_DIR_COLLAPS")
            .help("Prevents automatic collaps/skip of directory with single chapterized audio file")
            )
        .arg(Arg::new(ARG_IGNORE_CHAPTERS_META)
            .long(ARG_IGNORE_CHAPTERS_META)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_IGNORE_CHAPTERS_META")
            .help("Ignore chapters metadata, so files with chapters will not be presented as folders")
            )
        .arg(Arg::new(ARG_URL_PATH_PREFIX)
            .long(ARG_URL_PATH_PREFIX)
            .num_args(1)
            .value_parser(is_valid_url_path_prefix)
            .env("AUDIOSERVE_URL_PATH_PREFIX")
            .help("Base URL is a fixed path that is before audioserve path part, must start with / and not end with /  [default: none]")
            )
        .arg(Arg::new(ARG_FORCE_CACHE_UPDATE)
            .long(ARG_FORCE_CACHE_UPDATE)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_FORCE_CACHE_UPDATE")
            .help("Forces full reload of metadata cache on start")
            )
        .arg(Arg::new(ARG_STATIC_RESOURCE_CACHE_AGE)
            .long(ARG_STATIC_RESOURCE_CACHE_AGE)
            .env("AUDIOSERVE_STATIC_RESOURCE_CACHE_AGE")
            .num_args(1)
            .help("Age for Cache-Control of static resources, 'no-store' or number of secs, 0 means Cache-Control is not sent [default no-store]")
        )
        .arg(Arg::new(ARG_FOLDER_FILE_CACHE_AGE)
            .long(ARG_FOLDER_FILE_CACHE_AGE)
            .env("AUDIOSERVE_FOLDER_FILE_CACHE_AGE")
            .num_args(1)
            .help("Age for Cache-Control of cover and text files in audio folders, 'no-store' or number of secs, 0 means Cache-Control is not sent [default 1 day]")
        )
        .arg(
            Arg::new(ARG_COLLAPSE_CD_FOLDERS)
            .long(ARG_COLLAPSE_CD_FOLDERS)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_COLLAPSE_CD_FOLDERS")
            .help("Collapses multi CD folders into one root folder, CD subfolders are recognized by regular expression")
        )
        .arg(
            Arg::new(ARG_CD_FOLDER_REGEX)
            .long(ARG_CD_FOLDER_REGEX)
            .num_args(1)
            .requires(ARG_COLLAPSE_CD_FOLDERS)
            .env("AUDIOSERVE_CD_FOLDER_REGEX")
            .help("Regular expression to recognize CD subfolder, if want to use other then default")
        )
        .arg(
            Arg::new(ARG_ICONS_CACHE_DIR)
            .long(ARG_ICONS_CACHE_DIR)
            .num_args(1)
            .env("AUDIOSERVE_ICONS_CACHE_DIR")
            .value_parser(parent_dir_exists)
            .help("Directory for icons cache [default is ~/.audioserve/icons-cache]")
        ).arg(
            Arg::new(ARG_ICONS_CACHE_SIZE)
            .long(ARG_ICONS_CACHE_SIZE)
            .num_args(1)
            .env("AUDIOSERVE_ICONS_CACHE_SIZE")
            .value_parser(value_parser!(u32))
            .help("Max size of icons cache in MBi, when reached LRU items are deleted, [default is 100]")
        ).arg(
            Arg::new(ARG_ICONS_CACHE_MAX_FILES)
            .long(ARG_ICONS_CACHE_MAX_FILES)
            .num_args(1)
            .env("AUDIOSERVE_ICONS_CACHE_MAX_FILES")
            .value_parser(value_parser!(u64))
            .help("Max number of files in icons cache, when reached LRU items are deleted, [default is 1024]")
        ).arg(
            Arg::new(ARG_ICONS_CACHE_DISABLE)
            .long(ARG_ICONS_CACHE_DISABLE)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_ICONS_CACHE_DISABLE")
            .conflicts_with_all(&[ARG_ICONS_CACHE_SAVE_OFTEN, ARG_ICONS_CACHE_MAX_FILES, ARG_ICONS_CACHE_SIZE, ARG_ICONS_CACHE_DIR])
            .help("Icons cache is disabled.")
            )
        .arg(
            Arg::new(ARG_ICONS_CACHE_SAVE_OFTEN)
            .long(ARG_ICONS_CACHE_SAVE_OFTEN)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_ICONS_CACHE_SAVE_OFTEN")
            .help("Save additions to icons cache often, after each addition, this is normally not necessary")
        )
        .arg(
            Arg::new(ARG_ICONS_SIZE)
            .long(ARG_ICONS_SIZE)
            .num_args(1)
            .env("AUDIOSERVE_ICONS_SIZE")
            .value_parser(value_parser!(u32))
            .help("Size of folder icon in pixels, [default is 128]")
        )
        .arg(
            Arg::new(ARG_ICONS_FAST_SCALING)
            .long(ARG_ICONS_FAST_SCALING)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_ICONS_FAST_SCALING")
            .help("Use faster image scaling (linear triangle), by default slower, but better method (Lanczos3)")
        );

    // deprecated
    parser = parser.arg(Arg::new(ARG_SEARCH_CACHE).long(ARG_SEARCH_CACHE).help(
        "Deprecated: does nothing. For caching config use :<options> on individual collections dirs params",
    ));

    if cfg!(feature = "behind-proxy") {
        parser = parser.arg(Arg::new(ARG_BEHIND_PROXY)
        .long(ARG_BEHIND_PROXY)
        .action(ArgAction::SetTrue)
        .value_parser(FalseyValueParser::new())
        .env("AUDIOSERVE_BEHIND_PROXY")
        .help("Informs program that it is behind remote proxy, now used only for logging (to get true remote client ip)")
        )
    }

    if cfg!(feature = "folder-download") {
        parser = parser.arg(
            Arg::new(ARG_DISABLE_FOLDER_DOWNLOAD)
                .long(ARG_DISABLE_FOLDER_DOWNLOAD)
                .action(ArgAction::SetTrue)
                .value_parser(FalseyValueParser::new())
                .env("AUDIOSERVE_DISABLE_FOLDER_DOWNLOAD")
                .help("Disables API point for downloading whole folder"),
        );
    }

    if cfg!(feature = "tls") {
        parser = parser.arg(Arg::new(ARG_SSL_KEY)
            .long(ARG_SSL_KEY)
            .num_args(1)
            .requires(ARG_SSL_CERT)
            .value_parser(is_existing_file)
            .env("AUDIOSERVE_SSL_KEY")
            .help("TLS/SSL private key in PEM format, https is used")
            )
            .arg(Arg::new(ARG_SSL_CERT)
            .long(ARG_SSL_CERT)
            .requires(ARG_SSL_KEY)
            .value_parser(is_existing_file)
            .env("AUDIOSERVE_SSL_CERT")
            )
            .arg(Arg::new(ARG_SSL_KEY_PASSWORD)
                .long(ARG_SSL_KEY_PASSWORD)
                .num_args(1)
                .env("AUDIOSERVE_SSL_KEY_PASSWORD")
                .help("Deprecated - for PEM key password is not needed, so it should not be encrypted - default from rustls")
            );
    }

    if cfg!(feature = "shared-positions") {
        parser = parser.arg(
            Arg::new(ARG_POSITIONS_BACKUP_FILE)
            .long(ARG_POSITIONS_BACKUP_FILE)
            .num_args(1)
            .value_parser(parent_dir_exists)
            .env("AUDIOSERVE_POSITIONS_BACKUP_FILE")
            .help("File to back up last listened positions (can be used to restore positions as well, so has two slightly different uses) [default is None]"),
        )
        .arg(
            Arg::new(ARG_POSITIONS_WS_TIMEOUT)
            .long(ARG_POSITIONS_WS_TIMEOUT)
            .value_parser(duration_secs)
            .env("AUDIOSERVE_POSITIONS_WS_TIMEOUT")
            .help("Timeout in seconds for idle websocket connection use for playback position sharing [default 600s]")
        )
        .arg(
            Arg::new(ARG_POSITIONS_RESTORE)
            .long(ARG_POSITIONS_RESTORE)
            .num_args(1)
            .value_parser(["legacy", "v1"])
            .env("AUDIOSERVE_POSITIONS_RESTORE")
            .requires(ARG_POSITIONS_BACKUP_FILE)
            .help("Restores positions from backup JSON file, value is version of file legacy is before audioserve v0.16,  v1 is current")
        )
        .arg(
            Arg::new(ARG_POSITIONS_BACKUP_SCHEDULE)
            .long(ARG_POSITIONS_BACKUP_SCHEDULE)
            .num_args(1)
            .env("AUDIOSERVE_POSITIONS_BACKUP_SCHEDULE")
            .requires(ARG_POSITIONS_BACKUP_FILE)
            .help("Sets regular schedule for backing up playback position - should be cron expression m h dom mon dow- minute (m), hour (h), day of month (dom), month (mon) day of week (dow)")
        );
    }

    if cfg!(feature = "symlinks") {
        parser = parser.arg(
            Arg::new(ARG_ALLOW_SYMLINKS)
                .long(ARG_ALLOW_SYMLINKS)
                .action(ArgAction::SetTrue)
                .value_parser(FalseyValueParser::new())
                .env("AUDIOSERVE_ALLOW_SYMLINKS")
                .help("Will follow symbolic/soft links in collections directories"),
        );
    }

    if cfg!(feature = "tags-encoding") {
        parser = parser.arg(
            Arg::new(ARG_TAGS_ENCODING)
                .num_args(1)
                .long(ARG_TAGS_ENCODING)
                .env("AUDIOSERVE_TAGS_ENCODING")
                .help(
                    "Alternate character encoding for audio tags metadata, if UTF8 decoding fails",
                ),
        )
    }

    if cfg!(feature = "transcoding-cache") {
        parser=parser.arg(
            Arg::new(ARG_T_CACHE_DIR)
            .long(ARG_T_CACHE_DIR)
            .num_args(1)
            .env("AUDIOSERVE_T_CACHE_DIR")
            .value_parser(parent_dir_exists)
            .help("Directory for transcoding cache [default is ~/.audioserve/audioserve-cache]")
        ).arg(
            Arg::new(ARG_T_CACHE_SIZE)
            .long(ARG_T_CACHE_SIZE)
            .num_args(1)
            .env("AUDIOSERVE_T_CACHE_SIZE")
            .value_parser(value_parser!(u32))
            .help("Max size of transcoding cache in MBi, when reached LRU items are deleted, [default is 1024]")
        ).arg(
            Arg::new(ARG_T_CACHE_MAX_FILES)
            .long(ARG_T_CACHE_MAX_FILES)
            .num_args(1)
            .env("AUDIOSERVE_T_CACHE_MAX_FILES")
            .value_parser(value_parser!(u32))
            .help("Max number of files in transcoding cache, when reached LRU items are deleted, [default is 1024]")
        ).arg(
            Arg::new(ARG_T_CACHE_DISABLE)
            .long(ARG_T_CACHE_DISABLE)
            .action(ArgAction::SetTrue)
            .value_parser(FalseyValueParser::new())
            .env("AUDIOSERVE_T_CACHE_DISABLE")
            .conflicts_with_all(&[ARG_T_CACHE_SAVE_OFTEN, ARG_T_CACHE_MAX_FILES, ARG_T_CACHE_SIZE, ARG_T_CACHE_DIR])
            .help("Transaction cache is disabled. If you want to completely get rid of it, compile without 'transcoding-cache'")
            )
        .arg(
            Arg::new(ARG_T_CACHE_SAVE_OFTEN)
            .long(ARG_T_CACHE_SAVE_OFTEN)
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

macro_rules! set_config {
    ($args:ident, $cfg: expr,  Some($arg:expr)) => {
        if let Some(n) = $args.remove_one($arg) {
            $cfg = Some(n);
        }
    };

    ($args:ident, $cfg: expr, $arg:expr) => {
        if let Some(n) = $args.remove_one($arg) {
            $cfg = n;
        }
    };
}

macro_rules! set_config_flag {
    ($args:ident, $cfg: expr, $arg:expr) => {
        if has_flag!($args, $arg) {
            $cfg = true;
        }
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

    if has_flag!(args, ARG_HELP_DIR_OPTIONS) {
        print_dir_options_help();
        exit(0);
    }

    if has_flag!(args, ARG_HELP_TAGS) {
        print_tags_help();
        exit(0);
    }

    if has_flag!(args, ARG_FEATURES) {
        println!("{}", FEATURES);
        exit(0);
    }

    if let Some(dir) = args.get_one::<PathBuf>(ARG_DATA_DIR) {
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
                    ARG_DATA_DIR,
                    "Audioserve data directory {:?} cannot be created due to error {}",
                    d,
                    e
                )
            })?
        }
    }

    let mut no_authentication_confirmed = false;

    let mut config: Config = if let Some(config_file) = args.get_one::<PathBuf>(ARG_CONFIG) {
        let f = File::open(config_file).or_else(|e| {
            arg_error!(
                ARG_CONFIG,
                "Cannot open config file {:?}, error: {}",
                config_file,
                e
            )
        })?;

        serde_yaml::from_reader(f).or_else(|e| {
            arg_error!(
                ARG_CONFIG,
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

    if has_flag!(args, ARG_DEBUG) {
        let name = "RUST_LOG";
        if env::var_os(name).is_none() {
            env::set_var(name, ARG_DEBUG);
        }
    }

    if let Some(base_dirs) = args.get_many::<String>(ARG_BASE_DIR) {
        for dir in base_dirs {
            config.add_base_dir(dir)?;
        }
    }

    if let Ok(port) = env::var("PORT") {
        // this is hack for heroku, which requires program to use env. variable PORT
        let port: u16 = port
            .parse()
            .or_else(|_| arg_error!(ARG_LISTEN, "Invalid value in $PORT"))?;
        config.listen = SocketAddr::from(([0, 0, 0, 0], port));
    } else if let Some(addr) = args.remove_one::<SocketAddr>(ARG_LISTEN) {
        config.listen = addr;
    }

    if has_flag!(args, ARG_THREAD_POOL_LARGE) {
        config.thread_pool = ThreadPoolConfig {
            num_threads: 16,
            queue_size: 1000,
            keep_alive: None,
        };
    }

    if has_flag!(args, ARG_NO_AUTHENTICATION) {
        config.shared_secret = None;
        no_authentication_confirmed = true
    } else if let Some(secret) = args.remove_one(ARG_SHARED_SECRET) {
        config.shared_secret = Some(secret)
    } else if let Some(file) = args.remove_one::<PathBuf>(ARG_SHARED_SECRET_FILE) {
        config.set_shared_secret_from_file(file)?
    };

    set_config!(args, config.limit_rate, Some(ARG_LIMIT_RATE));
    set_config!(
        args,
        config.transcoding.max_parallel_processes,
        ARG_TRANSCODING_MAX_PARALLEL_PROCESSES
    );
    set_config!(
        args,
        config.transcoding.max_runtime_hours,
        ARG_TRANSCODING_MAX_RUNTIME
    );

    set_config!(
        args,
        config.thread_pool.keep_alive,
        Some(ARG_THREAD_POOL_KEEP_ALIVE_SECS)
    );

    if let Some(validity) = args.remove_one::<u32>(ARG_TOKEN_VALIDITY_DAYS) {
        config.token_validity_hours = validity * 24
    }
    set_config!(args, config.client_dir, ARG_CLIENT_DIR);
    set_config!(args, config.secret_file, ARG_SECRET_FILE);

    if has_flag!(args, ARG_CORS) {
        config.cors = match args.remove_one(ARG_CORS_REGEX) {
            Some(o) => Some(CorsConfig {
                inner: Cors::default(),
                regex: Some(o),
            }),
            None => Some(CorsConfig::default()),
        }
    }

    if has_flag!(args, ARG_COLLAPSE_CD_FOLDERS) {
        config.collapse_cd_folders = match args.remove_one(ARG_CD_FOLDER_REGEX) {
            Some(re) => Some(CollapseCDFolderConfig { regex: Some(re) }),
            None => Some(CollapseCDFolderConfig::default()),
        }
    }
    set_config_flag!(
        args,
        config.force_cache_update_on_init,
        ARG_FORCE_CACHE_UPDATE
    );

    if let Some(tags) = args.remove_many(ARG_TAGS_CUSTOM) {
        for t in tags {
            if !ALLOWED_TAGS.contains(&t) {
                arg_error!(ARG_TAGS_CUSTOM, "Unknown tag")?
            }
            config.tags.insert(t.to_string());
        }
    } else if has_flag!(args, ARG_TAGS) {
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

    if let Some(age) = args.remove_one(ARG_STATIC_RESOURCE_CACHE_AGE) {
        config.static_resource_cache_age = parse_cache_age(age)?;
    }

    if let Some(age) = args.remove_one(ARG_FOLDER_FILE_CACHE_AGE) {
        config.folder_file_cache_age = parse_cache_age(age)?;
    }

    set_config!(args, config.icons.size, ARG_ICONS_SIZE);
    set_config!(args, config.icons.cache_dir, ARG_ICONS_CACHE_DIR);
    set_config!(args, config.icons.cache_max_size, ARG_ICONS_CACHE_SIZE);
    set_config_flag!(args, config.icons.cache_disabled, ARG_ICONS_CACHE_DISABLE);
    set_config_flag!(args, config.icons.fast_scaling, ARG_ICONS_FAST_SCALING);
    set_config_flag!(
        args,
        config.icons.cache_save_often,
        ARG_ICONS_CACHE_SAVE_OFTEN
    );

    set_config!(args, config.url_path_prefix, Some(ARG_URL_PATH_PREFIX));

    set_config!(
        args,
        config.chapters.from_duration,
        ARG_CHAPTERS_FROM_DURATION
    );
    set_config!(args, config.chapters.duration, ARG_CHAPTERS_DURATION);
    set_config_flag!(args, config.no_dir_collaps, ARG_NO_DIR_COLLAPS);
    set_config_flag!(args, config.ignore_chapters_meta, ARG_IGNORE_CHAPTERS_META);

    // Arguments for optional features

    if cfg!(feature = "symlinks") && has_flag!(args, ARG_ALLOW_SYMLINKS) {
        config.allow_symlinks = true
    }

    #[cfg(feature = "tls")]
    {
        if let Some(key) = args.remove_one(ARG_SSL_KEY) {
            let key_file = key;
            let cert_file = args.remove_one(ARG_SSL_CERT).unwrap();
            config.ssl = Some(SslConfig {
                key_file,
                cert_file,
                key_password: "".into(),
            });
        }
    }

    #[cfg(feature = "transcoding-cache")]
    {
        set_config!(args, config.transcoding.cache.root_dir, ARG_T_CACHE_DIR);
        set_config!(args, config.transcoding.cache.max_size, ARG_T_CACHE_SIZE);
        set_config!(
            args,
            config.transcoding.cache.max_files,
            ARG_T_CACHE_MAX_FILES
        );
        set_config_flag!(args, config.transcoding.cache.disabled, ARG_T_CACHE_DISABLE);
        set_config_flag!(
            args,
            config.transcoding.cache.save_often,
            ARG_T_CACHE_SAVE_OFTEN
        );
    };

    if cfg!(feature = "folder-download") {
        set_config_flag!(
            args,
            config.disable_folder_download,
            ARG_DISABLE_FOLDER_DOWNLOAD
        );
    } else {
        config.disable_folder_download = true
    }

    if cfg!(feature = "behind-proxy") {
        set_config_flag!(args, config.behind_proxy, ARG_BEHIND_PROXY);
    } else {
        config.behind_proxy = false;
    }

    #[cfg(feature = "shared-positions")]
    {
        if let Some(ps) = args.remove_one(ARG_POSITIONS_RESTORE) {
            config.positions.restore = ps;
            no_authentication_confirmed = true;
        }
        set_config!(
            args,
            config.positions.backup_file,
            Some(ARG_POSITIONS_BACKUP_FILE)
        );
        set_config!(args, config.positions.ws_timeout, ARG_POSITIONS_WS_TIMEOUT);
        set_config!(
            args,
            config.positions.backup_schedule,
            Some(ARG_POSITIONS_BACKUP_SCHEDULE)
        );
    }

    #[cfg(feature = "tags-encoding")]
    {
        set_config!(args, config.tags_encoding, Some(ARG_TAGS_ENCODING));
    }

    if !no_authentication_confirmed && config.shared_secret.is_none() {
        return arg_error!(
            ARG_SHARED_SECRET,
            "Shared secret is None, but no authentication is not confirmed"
        );
    }

    config.check()?;
    config.prepare()?;
    if has_flag!(args, ARG_PRINT_CONFIG) {
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
    use std::path::Path;
    use std::time::Duration;
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
            "--positions-backup-file",
            "test_data/as-backup-json",
            "--positions-backup-schedule",
            "3 3 * * *",
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
