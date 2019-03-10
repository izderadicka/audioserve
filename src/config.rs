use super::services::transcode::{TranscodingFormat, QualityLevel, Transcoder};
use clap::{App, Arg};
use num_cpus;
use serde_yaml;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::path::{PathBuf,Path};

static mut CONFIG: Option<Config> = None;

// CONFIG is assured to be inited only once from main thread
pub fn get_config() -> &'static Config {
    unsafe { CONFIG.as_ref().expect("Config is not initialized") }
}

quick_error! {
#[derive(Debug)]
pub enum Error {

    InvalidNumber(err: ::std::num::ParseIntError) {
        from()
    }

    InvalidAddress(err: ::std::net::AddrParseError) {
        from()
    }

    InvalidPath(err: io::Error) {
        from()
    }

    InvalidLimitValue(err: &'static str) {
        from()
    }

    OtherError(err: String) {
        from()
    }

    InvalidYamlFile(err: serde_yaml::Error) {
        from()
    }

    NonExistentBaseDirectory{ }

    NonExistentClientDirectory{ }

    NonExistentSSLKeyFile { }
}
}

#[derive(Debug, Clone)]
pub struct TranscodingCacheConfig {
    pub root_dir: PathBuf,
    pub max_size: u64,
    pub max_files: u64,
    pub disabled: bool
}

impl Default for TranscodingCacheConfig {

    fn default() -> Self {
        let root_dir = env::temp_dir().join("audioserve-cache");
        TranscodingCacheConfig {
            root_dir,
            max_size: 1024*1024*1024,
            max_files: 1024,
            disabled: false
        }
    }
}

#[derive(Debug, Clone)]
pub struct TranscodingConfig {
    low: Option<TranscodingFormat>,
    medium: Option<TranscodingFormat>,
    high: Option<TranscodingFormat>,
}

impl Default for TranscodingConfig {
    fn default() -> Self {
        TranscodingConfig {
            low: None,
            medium: None,
            high: None,
        }
    }
}


impl TranscodingConfig {
    
    pub fn get(&self, quality: QualityLevel) -> TranscodingFormat {
        match quality {
            l @ QualityLevel::Low => self
                .low
                .as_ref()
                .map_or(TranscodingFormat::default_level(l), |c| c.clone()),
            l @ QualityLevel::Medium => self
                .medium
                .as_ref()
                .map_or(TranscodingFormat::default_level(l), |c| c.clone()),
            l @ QualityLevel::High => self
                .high
                .as_ref()
                .map_or(TranscodingFormat::default_level(l), |c| c.clone()),
            QualityLevel::Passthrough => TranscodingFormat::Remux
        }
    }
}

#[derive(Clone, Debug)]
pub struct ThreadPoolSize {
    pub num_threads: usize,
    pub queue_size: usize,
}

impl Default for ThreadPoolSize {
    fn default() -> Self {
        ThreadPoolSize {
            num_threads: 8,
            queue_size: 100,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChaptersSize {
    pub from_duration: usize,
    pub duration: usize
}

impl Default for ChaptersSize {
    fn default() -> Self {
        ChaptersSize {
            from_duration: 240,
            duration: 30
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub local_addr: SocketAddr,
    pub pool_size: ThreadPoolSize,
    pub base_dirs: Vec<PathBuf>,
    pub shared_secret: Option<String>,
    pub transcoding: TranscodingConfig,
    pub max_transcodings: usize,
    pub token_validity_hours: u64,
    pub secret_file: PathBuf,
    pub client_dir: PathBuf,
    pub cors: bool,
    pub ssl_key_file: Option<PathBuf>,
    pub ssl_key_password: Option<String>,
    pub allow_symlinks: bool,
    pub thread_keep_alive: Option<u32>,
    pub transcoding_deadline: u32,
    pub search_cache: bool,
    #[cfg(feature="transcoding-cache")]
    pub transcoding_cache: TranscodingCacheConfig,
    pub disable_folder_download: bool,
    #[cfg(feature="chapters")]
    pub chapters: ChaptersSize
}


impl Config {
    pub fn transcoder(&self, transcoding_quality: QualityLevel) -> Transcoder{
        Transcoder::new(get_config().transcoding.get(transcoding_quality))
    }
}
type Parser<'a> = App<'a, 'a>;

fn create_parser<'a>() -> Parser<'a> {
    let mut parser = App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .arg(Arg::with_name("debug")
            .short("d")
            .long("debug")
            .help("Enable debug logging (detailed logging config can be done via RUST_LOG env. variable)")
        )
        .arg(Arg::with_name("local_addr")
            .short("l")
            .long("listen")
            .help("Address and port server is listening on as address:port (by default listen on port 3000 on all interfaces)")
            .takes_value(true)
        )
        .arg(Arg::with_name("large-thread-pool")
            .long("large-thread-pool")
            .help("Use larger thread pool (usually will not be needed)")            
        )
        .arg(Arg::with_name("thread-keep-alive")
            .long("thread-keep-alive")
            .takes_value(true)
            .help("Thread in pool will shutdown after given seconds, if there is no work. Default is to keep threads forever.")
        )
        .arg(Arg::with_name("base_dir")
            .value_name("BASE_DIR")
            .required(true)
            .multiple(true)
            .min_values(1)
            .max_values(100)
            .takes_value(true)
            .help("Root directory for audio books")

        )
        .arg(Arg::with_name("no-authentication")
            .long("no-authentication")
            .help("no authentication required - mainly for testing purposes")
        )
        .arg(Arg::with_name("shared-secret")
            .short("s")
            .long("shared-secret")
            .takes_value(true)
            .conflicts_with("no-authentication")
            .required_unless_one(&["no-authentication", "shared-secret-file"])
            .help("Shared secret for client authentication")
        )
        .arg(Arg::with_name("shared-secret-file")
            .long("shared-secret-file")
            .takes_value(true)
            .conflicts_with("no-authentication")
            .required_unless("shared-secret")
            .help("File containing shared secret, it's slightly safer to read it from file, then provide as command argument")
        )
        .arg(Arg::with_name("transcoding-config")
            .long("transcoding-config")
            .takes_value(true)
            .help("Custom transcoding config in yaml file [defaul: built-in settings of 32, 48, 64 kbps")
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
            .help("Validity of authentication token issued by this server in days[default 365, min 10]")
            .default_value("365")
        )
        .arg(Arg::with_name("client-dir")
            .short("c")
            .long("client-dir")
            .takes_value(true)
            .help("Directory with client files - index.html and bundle.js")
            .default_value("./client/dist")
        )
        .arg(Arg::with_name("secret-file")
            .long("secret-file")
            .takes_value(true)
            .help("Path to file where server secret is kept - it's generated if it does not exists [default: is $HOME/.audioserve.secret]")
        )
        .arg(Arg::with_name("cors")
            .long("cors")
            .help("Enable CORS - enabled any origin of requests")
        );

    if cfg!(feature = "folder-download")   {
        parser = parser.arg(Arg::with_name("disable-folder-download")
                .long("disable-folder-download")
                .help("Disables API point for downloading whole folder")
        );
    }

    if cfg!(feature = "chapters") {
        parser = parser
        .arg(Arg::with_name("chapters-from-duration")
            .long("chapters-from-duration")
            .takes_value(true)
            .help("forces split of audio file larger then x mins into chapters (not physically, but it'll be just visible as folder with chapters)[default:240]")
        )
        .arg(Arg::with_name("chapters-duration")
            .long("chapters-duration")
            .takes_value(true)
            .help("If long files is presented as chapters, one chapter has x mins [default: 30]")
        )
    }

    if cfg!(feature = "tls") {
        parser = parser.arg(Arg::with_name("ssl-key")
            .long("ssl-key")
            .takes_value(true)
            .help("TLS/SSL private key and certificate in form of PKCS#12 key file, if provided, https is used")
            )
            .arg(Arg::with_name("ssl-key-password")
                .long("ssl-key-password")
                .takes_value(true)
                .requires("ssl-key")
                .help("Password for TLS/SSL private key")
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
    }

    parser
}

pub fn parse_args() -> Result<(), Error> {
    unsafe {
        if CONFIG.is_some() {
            panic!("Config is already initialied")
        }
    }

    let p = create_parser();
    let args = p.get_matches();

    if args.is_present("debug") {
        let name = "RUST_LOG";
        if env::var_os(name).is_none() {
            env::set_var(name, "debug");
        }
    }

    let base_dirs_items = args.values_of("base_dir").unwrap();
    let mut base_dirs = vec![];
    for dir in base_dirs_items {
        let base_dir: PathBuf = dir.into();
        if !base_dir.is_dir() {
            return Err(Error::NonExistentBaseDirectory);
        }
        base_dirs.push(base_dir);
    }

    let local_addr = if args.is_present("local_addr") {
        args.value_of("local_addr").unwrap().parse()? 
    } else {
        SocketAddr::from(([0,0,0,0], 
        env::var("PORT")
            .map_err(|_| ())
            .and_then(|v| v.parse().map_err(|_| error!("Invalid value in $PORT")))
            .unwrap_or(3000u16)))
    };

    let pool_size = if args.is_present("large-thread-pool") {
        ThreadPoolSize {
            num_threads: 16,
            queue_size: 1000,
        }
    } else {
        ThreadPoolSize::default()
    };

    let shared_secret = if args.is_present("no-authentication") {
        None
    } else {
        if args.is_present("shared-secret") {
            Some(args.value_of("shared-secret").unwrap().into())
        } else if args.is_present("shared-secret-file") {
            let p = Path::new(args.value_of_os("shared-secret-file").unwrap());
         match File::open(p) {
             Ok(mut f) => {
                 let mut secret = String::new();
                 f.read_to_string(&mut secret)
                 .map_err(|e| format!("Error reading from shared secret file: {}",e))?;
                 Some(secret)
             }
             Err(e) => {
                 return Err(format!("Shared secret file does not exists or is not readable: {}", e).into())
             }
         }
            
            
            
        
        } else {
            None
        }
    };

    let transcoding = match args.value_of("transcoding-config") {
        None => TranscodingConfig::default(),
        Some(f) => {
            let config_file = File::open(f)?;
            let mut qs: BTreeMap<String, TranscodingFormat> = serde_yaml::from_reader(config_file)?;
            TranscodingConfig {
                low: qs.remove("low"),
                medium: qs.remove("medium"),
                high: qs.remove("high"),
            }
        }
    };

    let max_transcodings = match args.value_of("max-transcodings") {
        Some(s) => s.parse()?,
        None => 2 * num_cpus::get(),
    };
    if max_transcodings < 1 {
        return Err("At least one concurrent trancoding must be available".into());
    } else if max_transcodings > 100 {
        return Err(
            "As transcodings are resource intesive, having more then 100 is not wise".into(),
        );
    }

    let transcoding_deadline = match args.value_of("transcoding-deadline").map(|x| x.parse()) {
        Some(Ok(0)) => return Err("transcoding-deadline must be positive".into()),
        Some(Err(_)) => return Err("invalid value for transcoding-deadline".into()),
        Some(Ok(x)) => x,
        None => 24,
    };

    let thread_keep_alive = match args.value_of("thread-keep-alive").map(|x| x.parse()) {
        Some(Ok(0)) => return Err("thread-keep-alive must be positive".into()),
        Some(Err(_)) => return Err("invalid value for thread-keep-alive".into()),
        Some(Ok(x)) => Some(x),
        None => None,
    };

    let token_validity_days: u64 = args.value_of("token-validity-days").unwrap().parse()?;
    if token_validity_days < 10 {
        return Err("Token must be valid for at least 10 days".into());
    }
    let client_dir: PathBuf = args.value_of("client-dir").unwrap().into();
    if !client_dir.exists() {
        return Err(Error::NonExistentClientDirectory);
    }

    let secret_file = match args.value_of("secret-file") {
        Some(s) => s.into(),
        None => match ::dirs::home_dir() {
            Some(home) => home.join(".audioserve.secret"),
            None => "./.audioserve.secret".into(),
        },
    };

    let cors = args.is_present("cors");
    let allow_symlinks = if cfg!(feature = "symlinks") {
        args.is_present("allow-symlinks")
    } else {
        false
    };

    let ssl_key_file;
    let ssl_key_password = if cfg!(feature = "tls") {
        ssl_key_file = match args.value_of("ssl-key") {
            Some(f) => {
                let p: PathBuf = f.into();
                if !p.exists() {
                    return Err(Error::NonExistentSSLKeyFile);
                }
                Some(p)
            }
            None => None,
        };

        args.value_of("ssl-key-password").map(|s| s.into())
    } else {
        ssl_key_file = None;
        None
    };

    let search_cache = if cfg!(feature = "search-cache") {
        args.is_present("search-cache")
    } else {
        false
    };

    let _transcoding_cache = if cfg!(feature="transcoding-cache") {
        let mut c = TranscodingCacheConfig::default();
        if let Some(d) = args.value_of("t-cache-dir") {
            c.root_dir = d.into()
        }

        if let Some(n) = args.value_of("t-cache-size") {
            let size: u64 = n.parse()?;
            if size < 50 {
                return Err("Cache smaller then 50Mbi does not make much sense".into())
            }
            c.max_size = 1024*1024 * size;
        }

        if let Some(n) = args.value_of("t-cache-max-files") {
            let num: u64 = n.parse()?;
            if num < 10 {
                return Err("Cache smaller then 10 files does not make much sense".into())
            }
            c.max_files = num;
        }

        if args.is_present("t-cache-disable") {
            c.disabled = true;
        }

        Some(c)
    } else {
        None
    };

    let disable_folder_download =  if cfg!(feature = "folder-download")   {
        args.is_present("disable-folder-download")
    } else {
        true
    };

    #[cfg(feature="chapters")]
    let chapters = {
        let mut c = ChaptersSize::default();
        let from_duration = args.value_of("chapters-from-duration")
            .and_then(|v| v.parse().ok());
        if let Some(from_duration) = from_duration {
            c.from_duration = from_duration
        }

        let duration = args.value_of("chapters-duration")
            .and_then(|v| v.parse().ok());
        if let Some(duration) = duration {
            if duration < 10 {
                return Err(Error::InvalidLimitValue("chapter should have at least 10 mins"))
            }
            c.duration = duration;
        }

        c
    };

    let config = Config {
        base_dirs,
        local_addr,
        pool_size,
        shared_secret,
        transcoding,
        max_transcodings,
        token_validity_hours: token_validity_days * 24,
        client_dir,
        secret_file,
        cors,
        ssl_key_file,
        ssl_key_password,
        allow_symlinks,
        thread_keep_alive,
        transcoding_deadline,
        search_cache,
        #[cfg(feature="transcoding-cache")]
        transcoding_cache: _transcoding_cache.unwrap(),
        disable_folder_download,
        #[cfg(feature="chapters")]
        chapters

    };
    unsafe {
        CONFIG = Some(config);
    }

    Ok(())
}

//this default config is used only for testing
#[allow(dead_code)]
pub fn init_default_config() {
    unsafe {
        if CONFIG.is_some() {
            return;
        }
    }

    let config = Config {
        base_dirs: vec![],
        local_addr: "127.0.0.1:3000".parse().unwrap(),
        pool_size: ThreadPoolSize::default(),
        shared_secret: None,
        transcoding: TranscodingConfig::default(),
        max_transcodings: 10,
        token_validity_hours: 365 * 24,
        client_dir: "./client/dist".into(),
        secret_file: "./secret".into(),
        cors: false,
        ssl_key_file: None,
        ssl_key_password: None,
        allow_symlinks: false,
        thread_keep_alive: None,
        transcoding_deadline: 24,
        search_cache: false,
        #[cfg(feature="transcoding-cache")]
        transcoding_cache: TranscodingCacheConfig::default(),
        disable_folder_download: false,
        #[cfg(feature="chapters")]
        chapters: ChaptersSize::default()
    };
    unsafe {
        CONFIG = Some(config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_serialize() {
        let mut qualities = BTreeMap::new();
        qualities.insert("low", TranscodingFormat::default_level(QualityLevel::Low));
        qualities.insert("medium", TranscodingFormat::default_level(QualityLevel::Medium));
        qualities.insert("high", TranscodingFormat::default_level(QualityLevel::High));
        let s = serde_yaml::to_string(&qualities).unwrap();
        assert!(s.len() > 20);
        println!("{}", s);

        let des: BTreeMap<String, TranscodingFormat> = serde_yaml::from_str(&s).unwrap();
        assert_eq!(des.get("medium"), qualities.get("medium"));
    }
    #[test]
    fn test_yaml_deserialize() {
        fn load_file(fname: &str) {
           let f = File::open(fname).unwrap();
            let des: BTreeMap<String, TranscodingFormat> = serde_yaml::from_reader(f).unwrap();
            assert_eq!(3, des.len());
            assert!(des.get("high").is_some()); 
        }
        load_file("./test_data/transcodings.yaml");
        load_file("./test_data/transcodings.1.yaml");
        load_file("./test_data/transcodings.2.yaml");
    }

}
