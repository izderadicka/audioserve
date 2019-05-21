use super::services::transcode::{QualityLevel, Transcoder, TranscodingFormat};

use num_cpus;
use serde_yaml;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use self::error::{Error, Result};

mod cli;
mod validators;
#[macro_use]
mod error;

static mut CONFIG: Option<Config> = None;

// CONFIG is assured to be inited only once from main thread
pub fn get_config() -> &'static Config {
    unsafe { CONFIG.as_ref().expect("Config is not initialized") }
}

#[cfg(feature = "transcoding-cache")]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TranscodingCacheConfig {
    pub root_dir: PathBuf,
    pub max_size: u64,
    pub max_files: u32,
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

#[cfg(feature = "transcoding-cache")]
impl TranscodingCacheConfig {
    pub fn set_root_dir<P: Into<PathBuf>>(&mut self, root_dir: P) -> Result<()> {
        let d = root_dir.into();
        if let Some(true) = d.parent().map(Path::is_dir) {
            self.root_dir = d;
            Ok(())
        } else {
            value_error!("root_dir", "Parent directory does not exists for {:?}", d)
        }
    }

    pub fn set_max_size_mb(&mut self, sz: u32) -> Result<()> {
        if sz < 50 {
            return value_error!(
                "max_size",
                "Transcoding cache small then 50 MB does not make sense"
            );
        }
        self.max_size = sz as u64 * 1024 * 1024;
        Ok(())
    }

    pub fn set_max_files(&mut self, sz: u32) -> Result<()> {
        if sz < 10 {
            return value_error!(
                "max_size",
                "Transcoding cache with less the 10 files does not make sense"
            );
        }
        self.max_files = sz;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TranscodingConfig {
    pub max_parallel_processes: u32,
    pub max_runtime_hours: u32,
    low: TranscodingFormat,
    medium: TranscodingFormat,
    high: TranscodingFormat,
}

impl Default for TranscodingConfig {
    fn default() -> Self {
        TranscodingConfig {
            max_parallel_processes: (2 * num_cpus::get()) as u32,
            max_runtime_hours: 24,
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

    pub fn set_max_parallel_processes(&mut self, n:u32) -> Result<()> {
        if n < 2 {
            return value_error!("max_parallel_processes", "With less then 2 transcoding processes audioserve will not work properly")
        } else if n > 100 {
             return value_error!("max_parallel_processes", "As transcodings are resource intesive, having more then 100 is not wise")
        }
        self.max_parallel_processes = n;
        Ok(())

    }

    pub fn set_max_runtime_hours(&mut self, n:u32) -> Result<()> {
        if n<1 {
            return value_error!("max_runtime_hours", "Minimum time is 1 hour")
        }
        self.max_runtime_hours = n;
        Ok(())   
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ThreadPoolConfig {
    pub num_threads: u16,
    pub queue_size: u16,
    pub keep_alive: Option<Duration>,
}

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        ThreadPoolConfig {
            num_threads: 8,
            queue_size: 100,
            keep_alive: None,
        }
    }
}

impl ThreadPoolConfig {
    pub fn set_num_threads(&mut self, n: u16) -> Result<()> {
        if n < 1 {
            return value_error!("num_threads", "At least one thread is required");
        }
        if n > 32_768 {
            return value_error!(
                "num_threads",
                "{} is just too many threads, max is 32768",
                n
            );
        }
        self.num_threads = n;
        Ok(())
    }
    pub fn set_queue_size(&mut self, n: u16) -> Result<()> {
        if n < 10 {
            return value_error!(
                "queue_size",
                "Queue for blocking threads should be at least 10 "
            );
        }
        if n > 32_768 {
            return value_error!(
                "queue_size",
                "{} is just too long for queue of blocking threads, max is 32768",
                n
            );
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

impl ChaptersSize {
    pub fn set_from_duration(&mut self, d:u32) -> Result<()> {
        if d > 0 && d < 10 {
            return value_error!("from_duration", "File shorter then 10 mins should not be split to chapters")
        }

        self.from_duration = d;
        Ok(())
    }

    pub fn set_duration(&mut self, d: u32) -> Result<()> {
        if d < 10 {
            return value_error!("duration", "Minimal chapter duration is 10 minutes")
        }

        self.duration = d;
        Ok(())
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
    pub token_validity_hours: u32,
    pub secret_file: PathBuf,
    pub client_dir: PathBuf,
    pub cors: bool,
    pub ssl: Option<SslConfig>,
    pub allow_symlinks: bool,
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

    pub fn add_base_dir<P: Into<PathBuf>>(&mut self, p: P) -> Result<()> {
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
        if secret.len() < 3 {
            return value_error!("shared_secret", "Shared secret must be at least 3 bytes");
        }
        self.shared_secret = Some(secret);
        Ok(())
    }

    pub fn set_shared_secret_from_file<P: AsRef<Path> + std::fmt::Debug>(
        &mut self,
        file: P,
    ) -> Result<()> {
        match File::open(&file) {
            Ok(mut f) => {
                let mut secret = String::new();
                f.read_to_string(&mut secret).or_else(|e| {
                    value_error!(
                        "shared_secret",
                        "Error reading from shared secret file {:?}: {}",
                        file,
                        e
                    )
                })?;
                self.set_shared_secret(secret)
            }
            Err(e) => value_error!(
                "shared-secret",
                "Shared secret file {:?} does not exists or is not readable: {}",
                file,
                e
            ),
        }
    }

    pub fn set_token_validity_days(&mut self, validity: u32) -> Result<()> {
        if validity < 10 {
            return value_error!(
                "token-validity-days",
                "Token must be valid for at least 10 days"
            );
        }
        self.token_validity_hours = validity * 24;
        Ok(())
    }

    pub fn set_client_dir<P: Into<PathBuf>>(&mut self, dir: P) -> Result<()> {
        let p = dir.into();
        if !p.is_dir() {
            return value_error!(
                "client_dir",
                "Directory with web client files {:?} does not exists or is not directory",
                p
            );
        }
        self.client_dir = p;
        Ok(())
    }

    pub fn set_secret_file<P: Into<PathBuf>>(&mut self, secret_file: P) -> Result<()> {
        let f = secret_file.into();
        if let Some(true) = f.parent().map(Path::is_dir) {
            self.secret_file = f;
            Ok(())
        } else {
            value_error!(
                "secret_file",
                "Parent directory for does not exists for {:?}",
                f
            )
        }
    }

    pub fn set_ssl_config(&mut self, ssl: SslConfig) -> Result<()> {
        if !ssl.key_file.is_file() {
            return value_error!("ssl", "SSL key file {:?} doesn't exist", ssl.key_file);
        }
        self.ssl = Some(ssl);
        Ok(())
    }

    pub fn set_positions_file<P: Into<PathBuf>>(&mut self, file:P) -> Result<()> {
        let f = file.into();
        if let Some(true) = f.parent().map(Path::is_dir) {
            self.positions_file = f;
            Ok(())
        } else {
            value_error!(
                "positions_file",
                "Parent directory for does not exists for {:?}",
                f
            )
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| ".".into());
        Config {
            base_dirs: vec![],
            local_addr: ([0, 0, 0, 0], 3000u16).into(),
            thread_pool: ThreadPoolConfig::default(),
            shared_secret: None,
            transcoding: TranscodingConfig::default(),
            token_validity_hours: 365 * 24,
            client_dir: "client/dist".into(),
            secret_file: home.join(".audioserve.secret"),
            cors: false,
            ssl: None,
            allow_symlinks: false,
            search_cache: false,
            #[cfg(feature = "transcoding-cache")]
            transcoding_cache: TranscodingCacheConfig::default(),
            disable_folder_download: false,
            chapters: ChaptersSize::default(),
            positions_file: home.join(".audioserve-positions"),
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
