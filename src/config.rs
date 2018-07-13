use super::services::transcode::{Quality, QualityLevel};
use clap::{App, Arg};
use num_cpus;
use serde_yaml;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

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

    InvalidYamlFile(err: serde_yaml::Error) {
        from()
    }

    NonExistentBaseDirectory{ }

    NonExistentClientDirectory{ }

    NonExistentSSLKeyFile { }
}
}

#[derive(Debug, Clone)]
pub struct TranscodingConfig {
    low: Option<Quality>,
    medium: Option<Quality>,
    high: Option<Quality>,
}

impl TranscodingConfig {
    fn default() -> Self {
        TranscodingConfig {
            low: None,
            medium: None,
            high: None,
        }
    }
    pub fn get(&self, quality: QualityLevel) -> Quality {
        match quality {
            l @ QualityLevel::Low => self
                .low
                .as_ref()
                .map_or(Quality::default_level(l), |c| c.clone()),
            l @ QualityLevel::Medium => self
                .medium
                .as_ref()
                .map_or(Quality::default_level(l), |c| c.clone()),
            l @ QualityLevel::High => self
                .high
                .as_ref()
                .map_or(Quality::default_level(l), |c| c.clone()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ThreadPoolSize {
    pub min_threads: usize,
    pub max_threads: usize,
    pub queue_size: usize,
}

impl ThreadPoolSize {
    fn default() -> Self {
        ThreadPoolSize {
            min_threads: 4,
            max_threads: 8,
            queue_size: 100,
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
            .help("Address and port server is listening on as address:port")
            .takes_value(true)
            .default_value("0.0.0.0:3000")
        )
        .arg(Arg::with_name("large-thread-pool")
            .long("large-thread-pool")
            .help("Use larger thread pool (usually will not be needed)")            
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
            .required_unless("no-authentication")
            .help("Shared secret for client authentication")
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
            .help("Path to file where server is kept - it's generated if it does not exists [default: is $HOME/.audioserve.secret]")
        )
        .arg(Arg::with_name("cors")
            .long("cors")
            .help("Enable CORS - enables any origin of requests")
        );

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
                .help("Will follow symbolic/sof links in collections directories"),
        );
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

    let local_addr = args.value_of("local_addr").unwrap().parse()?;

    let pool_size = if args.is_present("large-thread-pool") {
        ThreadPoolSize {
            min_threads: 8,
            max_threads: 32,
            queue_size: 1000,
        }
    } else {
        ThreadPoolSize::default()
    };

    let shared_secret = if args.is_present("no-authentication") {
        None
    } else {
        Some(args.value_of("shared-secret").unwrap().into())
    };

    let transcoding = match args.value_of("transcoding-config") {
        None => TranscodingConfig::default(),
        Some(f) => {
            let config_file = File::open(f)?;
            let mut qs: BTreeMap<String, Quality> = serde_yaml::from_reader(config_file)?;
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
        None => match ::std::env::home_dir() {
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
        qualities.insert("low", Quality::default_level(QualityLevel::Low));
        qualities.insert("medium", Quality::default_level(QualityLevel::Medium));
        qualities.insert("high", Quality::default_level(QualityLevel::High));
        let s = serde_yaml::to_string(&qualities).unwrap();
        assert!(s.len() > 20);
        println!("{}", s);

        let des: BTreeMap<String, Quality> = serde_yaml::from_str(&s).unwrap();
        assert_eq!(des.get("medium"), qualities.get("medium"));
    }
    #[test]
    fn test_yaml_deserialize() {
        let f = File::open("./test_data/transcodings.yaml").unwrap();
        let des: BTreeMap<String, Quality> = serde_yaml::from_reader(f).unwrap();
        assert_eq!(3, des.len());
        assert!(des.get("high").is_some());
    }

}
