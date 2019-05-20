use super::services::transcode::{QualityLevel, Transcoder, TranscodingFormat};

use num_cpus;
use serde_yaml;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::fmt::{self,Display};
use std::borrow::Cow;
use std::time::Duration;


mod cli;
mod validators;

static mut CONFIG: Option<Config> = None;

// CONFIG is assured to be inited only once from main thread
pub fn get_config() -> &'static Config {
    unsafe { CONFIG.as_ref().expect("Config is not initialized") }
}

#[derive(Debug)]
pub enum ErrorKind {
    Argument{argument:&'static str, message: Cow<'static, str>},
    ConfigValue{name:&'static str, message: Cow<'static, str>}
}

#[derive(Debug)]
pub struct Error(ErrorKind);

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            ErrorKind::Argument{argument, ref message} => {
                write!(f, "Error in argument {}: {}", argument, message)
            }
            ErrorKind::ConfigValue{name, ref message} => {
                write!(f, "Error in config value {}: {}", name, message)
            }
        }
    }
}

impl std::error::Error for Error {}

type Result<T> =  std::result::Result<T,Error>;

impl Error {
    fn in_argument<T,S>(argument: &'static str, msg: S) -> std::result::Result<T,Self>
    where S: Into<Cow<'static, str>> {
        Err(Error(
            ErrorKind::Argument {
                argument,
                message: msg.into()
            }
        ))
    }

    fn in_value<T,S>(name: &'static str, msg: S) -> std::result::Result<T,Self>
    where S: Into<Cow<'static, str>> {
        Err(Error(
            ErrorKind::ConfigValue {
                name,
                message: msg.into()
            }
        ))
    }
}

macro_rules!  value_error {
    ($arg:expr, $msg:expr) => {
        Error::in_value($arg, $msg)
    };

    ($arg:expr, $msg:expr, $($param:expr),+) => {
        Error::in_value($arg, 
        format!($msg, $($param),+))
    };

}


#[cfg(feature = "transcoding-cache")]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TranscodingCacheConfig {
    pub root_dir: PathBuf,
    pub max_size: u64,
    pub max_files: u64,
    pub disabled: bool,
    pub save_often: bool,
}

#[cfg(feature = "transcoding-cache")]
impl Default for TranscodingCacheConfig {
    fn default() -> Self {
        let root_dir = env::temp_dir().join("audioserve-cache");
        TranscodingCacheConfig {
            root_dir,
            max_size: 1024 * 1024 * 1024,
            max_files: 1024,
            disabled: false,
            save_often: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TranscodingConfig {
    low: TranscodingFormat,
    medium: TranscodingFormat,
    high: TranscodingFormat,
}

impl Default for TranscodingConfig {
    fn default() -> Self {
        TranscodingConfig {
            low: TranscodingFormat::default_level(QualityLevel::Low),
            medium: TranscodingFormat::default_level(QualityLevel::Medium),
            high: TranscodingFormat::default_level(QualityLevel::High),
        }
    }
}

impl TranscodingConfig {
    pub fn get(&self, quality: QualityLevel) -> TranscodingFormat {
        match quality {
            QualityLevel::Low => self.low.clone(),
            QualityLevel::Medium => self.medium.clone(),
            QualityLevel::High => self.high.clone(),
            QualityLevel::Passthrough => TranscodingFormat::Remux,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ThreadPoolConfig {
    pub num_threads: u16,
    pub queue_size: u16,
    pub keep_alive: Option<Duration>
}

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        ThreadPoolConfig {
            num_threads: 8,
            queue_size: 100,
            keep_alive: None
        }
    }
}

impl ThreadPoolConfig {
    pub fn set_num_threads(&mut self, n: u16) -> Result<()> {
         if n<1 {
            return value_error!("num_threads", "At least one thread is required")
        }
        if n > 32_768 {
            return value_error!("num_threads", "{} is just too many threads, max is 32768", n)
        }
        self.num_threads = n;
        Ok(())
    }
    pub fn set_queue_size(&mut self, n: u16) -> Result<()> {

        if n < 10 {
            return value_error!("queue_size", "Queue for blocking threads should be at least 10 ")
        }
        if n > 32_768 {
            return value_error!("queue_size", "{} is just too long for queue of blocking threads, max is 32768",n)
        }
        self.queue_size = n;
        Ok(())
    }

    pub fn set_keep_alive_secs(&mut self, secs: u32) -> Result<()> {
        if secs == 0 {
            self.keep_alive = None
        } else {
            self.keep_alive = Some(Duration::from_secs(secs as u64))
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChaptersSize {
    pub from_duration: u32,
    pub duration: u32,
}

impl Default for ChaptersSize {
    fn default() -> Self {
        ChaptersSize {
            from_duration: 0,
            duration: 30,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SslConfig {
    pub key_file: PathBuf,
    pub key_password: String,
}


#[derive(Debug, Clone)]
pub struct Config {
    pub local_addr: SocketAddr,
    pub thread_pool: ThreadPoolConfig,
    pub base_dirs: Vec<PathBuf>,
    pub shared_secret: Option<String>,
    pub transcoding: TranscodingConfig,
    pub max_transcodings: usize,
    pub token_validity_hours: u32,
    pub secret_file: PathBuf,
    pub client_dir: PathBuf,
    pub cors: bool,
    pub ssl: Option<SslConfig>,
    pub allow_symlinks: bool,
    pub transcoding_deadline: u32,
    pub search_cache: bool,
    #[cfg(feature = "transcoding-cache")]
    pub transcoding_cache: TranscodingCacheConfig,
    pub disable_folder_download: bool,
    pub chapters: ChaptersSize,
    pub positions_file: PathBuf,
}


impl Config {
    pub fn transcoder(&self, transcoding_quality: QualityLevel) -> Transcoder {
        Transcoder::new(get_config().transcoding.get(transcoding_quality))
    }

    pub fn add_base_dir<P:Into<PathBuf>>(&mut self, p:P) -> Result<()> {
        let base_dir = p.into();
        if !base_dir.is_dir() {
            return value_error!("base_dir", "{:?} is not direcrory", base_dir);
        }
        self.base_dirs.push(base_dir);
        Ok(())
    }

    pub fn set_pool_size(&mut self, v: ThreadPoolConfig) -> Result<()> {
        self.thread_pool.set_num_threads(v.num_threads)?;
        self.thread_pool.set_queue_size(v.queue_size)?;
        Ok(())
    }

    pub fn set_shared_secret(&mut self, secret: String) -> Result<()> {
        if secret.len()<3 {
            return value_error!("shared_secret", "Shared secret must be at least 3 bytes")
        }
        self.shared_secret = Some(secret);
        Ok(())
    }

    pub fn set_shared_secret_from_file< P:AsRef<Path>+std::fmt::Debug>(&mut self, file: P) -> Result<()> {
        match File::open(&file) {
            Ok(mut f) => {
                let mut secret = String::new();
                f.read_to_string(&mut secret)
                    .or_else(|e| value_error!("shared_secret", "Error reading from shared secret file {:?}: {}", file, e))?;
                self.set_shared_secret(secret)
            }
            Err(e) => {
                value_error!(
                    "shared-secret",
                    "Shared secret file {:?} does not exists or is not readable: {}",
                    file,
                    e
                )
            }
        }

    }

    pub fn set_token_validity_days(&mut self, validity: u32) -> Result<()> {
        if validity < 10 { 
            return value_error!("token-validity-days",
            "Token must be valid for at least 10 days")
        }
        self.token_validity_hours = validity * 24;
        Ok(())
    }

    pub fn set_client_dir<P:Into<PathBuf>>(&mut self, dir: P) -> Result<()> {
        let p = dir.into();
        if !p.is_dir() {
            return value_error!("client_dir", "Directory with web client files {:?} does not exists or is not directory", p);
        }
        self.client_dir = p;
        Ok(())
    }

    pub fn set_secret_file<P:Into<PathBuf>>(&mut self, secret_file:P) -> Result<()> {
        let f = secret_file.into();
        if let Some(true) = f.parent().map(Path::is_dir) {
            self.secret_file = f;
            Ok(())
        } else {
            value_error!("secret_file", "Parent directory for does not exists for {:?}", f)
        }
    }

    pub fn set_ssl_config(&mut self, ssl: SslConfig) -> Result<()> {
        if !ssl.key_file.is_file() {
            return value_error!("ssl", "SSL key file {:?} doesn't exist", ssl.key_file)
        }
        self.ssl = Some(ssl);
        Ok(())
    }




}

impl Default for Config {
    fn default() -> Self {
        Config {
        base_dirs: vec![],
        local_addr: ([0,0,0,0], 3000u16).into(),
        thread_pool: ThreadPoolConfig::default(),
        shared_secret: None,
        transcoding: TranscodingConfig::default(),
        max_transcodings: 2 * num_cpus::get(),
        transcoding_deadline: 24,
        token_validity_hours: 365 * 24,
        client_dir: "client/dist".into(),
        secret_file: match dirs::home_dir() {
            Some(home) => home.join(".audioserve.secret"),
            None => "./.audioserve.secret".into()
            },
        cors: false,
        ssl: None,
        allow_symlinks: false,
        search_cache: false,
        #[cfg(feature = "transcoding-cache")]
        transcoding_cache: TranscodingCacheConfig::default(),
        disable_folder_download: false,
        chapters: ChaptersSize::default(),
        positions_file: "./positions".into(),
    }

    }
}

pub fn init_config() -> Result<()> {
    unsafe {
        if CONFIG.is_some() {
            panic!("Config is already initialied")
        }
    }
    let config = Config::default();
    let config = cli::parse_args(config)?;

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
    let config = Config::default();
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
        qualities.insert(
            "medium",
            TranscodingFormat::default_level(QualityLevel::Medium),
        );
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
