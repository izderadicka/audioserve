pub use self::error::{Error, Result};
use super::services::transcode::{QualityLevel, Transcoder, TranscodingFormat};
use crate::util;
use std::env;
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod cli;
mod validators;
#[macro_use]
mod error;

static mut CONFIG: Option<Config> = None;

// CONFIG is assured to be inited only once from main thread
pub fn get_config() -> &'static Config {
    unsafe { CONFIG.as_ref().expect("Config is not initialized") }
}

static mut BASE_DATA_DIR: Option<PathBuf> = None;

fn base_data_dir() -> &'static PathBuf {
    unsafe {
        BASE_DATA_DIR
            .as_ref()
            .expect("Base data dir is not initialized")
    }
}

#[cfg(feature = "transcoding-cache")]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TranscodingCacheConfig {
    pub root_dir: PathBuf,
    pub max_size: u32,
    pub max_files: u32,
    pub disabled: bool,
    pub save_often: bool,
}

#[cfg(feature = "transcoding-cache")]
impl Default for TranscodingCacheConfig {
    fn default() -> Self {
        let data_base_dir = base_data_dir();
        let root_dir = data_base_dir.join("audioserve-cache");
        TranscodingCacheConfig {
            root_dir,
            max_size: 1024,
            max_files: 1024,
            disabled: false,
            save_often: false,
        }
    }
}

#[cfg(feature = "transcoding-cache")]
impl TranscodingCacheConfig {
    pub fn check(&self) -> Result<()> {
        if !util::parent_dir_exists(&self.root_dir) {
            return value_error!(
                "root_dir",
                "Parent directory does not exists for {:?}",
                self.root_dir
            );
        };

        if self.max_size < 50 {
            return value_error!(
                "max_size",
                "Transcoding cache small then 50 MB does not make sense"
            );
        }

        if self.max_files < 10 {
            return value_error!(
                "max_size",
                "Transcoding cache with less the 10 files does not make sense"
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TranscodingConfig {
    pub max_parallel_processes: usize,
    pub max_runtime_hours: u32,
    #[cfg(feature = "transcoding-cache")]
    pub cache: TranscodingCacheConfig,
    low: TranscodingFormat,
    medium: TranscodingFormat,
    high: TranscodingFormat,
}

impl Default for TranscodingConfig {
    fn default() -> Self {
        TranscodingConfig {
            max_parallel_processes: (2 * num_cpus::get()),
            max_runtime_hours: 24,
            #[cfg(feature = "transcoding-cache")]
            cache: TranscodingCacheConfig::default(),
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

    pub fn check(&self) -> Result<()> {
        if self.max_parallel_processes < 2 {
            return value_error!(
                "max_parallel_processes",
                "With less then 2 transcoding processes audioserve will not work properly"
            );
        } else if self.max_parallel_processes > 100 {
            return value_error!(
                "max_parallel_processes",
                "As transcodings are resource intesive, having more then 100 is not wise"
            );
        }

        if self.max_runtime_hours < 1 {
            return value_error!("max_runtime_hours", "Minimum time is 1 hour");
        }
        #[cfg(feature = "transcoding-cache")]
        self.cache.check()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
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
    pub fn check(&self) -> Result<()> {
        if self.num_threads < 1 {
            return value_error!("num_threads", "At least one thread is required");
        }
        if self.num_threads > 32_768 {
            return value_error!(
                "num_threads",
                "{} is just too many threads, max is 32768",
                self.num_threads
            );
        }

        if self.queue_size < 10 {
            return value_error!(
                "queue_size",
                "Queue for blocking threads should be at least 10 "
            );
        }
        if self.queue_size > 32_768 {
            return value_error!(
                "queue_size",
                "{} is just too long for queue of blocking threads, max is 32768",
                self.queue_size
            );
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
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
    pub fn check(&self) -> Result<()> {
        if self.from_duration > 0 && self.from_duration < 10 {
            return value_error!(
                "from_duration",
                "File shorter then 10 mins should not be split to chapters"
            );
        }

        if self.duration < 10 {
            return value_error!("duration", "Minimal chapter duration is 10 minutes");
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SslConfig {
    pub key_file: PathBuf,
    pub key_password: String,
}

impl SslConfig {
    pub fn check(&self) -> Result<()> {
        if !self.key_file.is_file() {
            return value_error!("ssl", "SSL key file {:?} doesn't exist", self.key_file);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub listen: SocketAddr,
    pub thread_pool: ThreadPoolConfig,
    pub base_dirs: Vec<PathBuf>,
    pub url_path_prefix: Option<String>,
    pub shared_secret: Option<String>,
    pub limit_rate: Option<f32>,
    pub transcoding: TranscodingConfig,
    pub token_validity_hours: u32,
    pub secret_file: PathBuf,
    pub client_dir: PathBuf,
    pub cors: bool,
    pub ssl: Option<SslConfig>,
    pub allow_symlinks: bool,
    pub search_cache: bool,
    pub disable_folder_download: bool,
    pub chapters: ChaptersSize,
    pub no_dir_collaps: bool,
    pub ignore_chapters_meta: bool,
    pub positions_file: PathBuf,
    pub positions_ws_timeout: Duration,
    pub behind_proxy: bool,
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
                self.shared_secret = Some(secret);
                Ok(())
            }
            Err(e) => value_error!(
                "shared-secret",
                "Shared secret file {:?} does not exists or is not readable: {}",
                file,
                e
            ),
        }
    }

    pub fn check(&self) -> Result<()> {
        if self
            .shared_secret
            .as_ref()
            .map(String::len)
            .unwrap_or(std::usize::MAX)
            < 3
        {
            return value_error!("shared_secret", "Shared secret must be at least 3 bytes");
        }

        if self.token_validity_hours < 240 {
            return value_error!(
                "token-validity-days",
                "Token must be valid for at least 10 days"
            );
        }

        if self.positions_ws_timeout < Duration::from_secs(60) {
            return value_error!("positions-ws-timeout", "Timeout must be at least 60s");
        }

        if !self.client_dir.is_dir() {
            return value_error!(
                "client_dir",
                "Directory with web client files {:?} does not exists or is not directory",
                self.client_dir
            );
        }

        if !util::parent_dir_exists(&self.secret_file) {
            return value_error!(
                "secret_file",
                "Parent directory for does not exists for {:?}",
                self.secret_file
            );
        };

        if !util::parent_dir_exists(&self.positions_file) {
            return value_error!(
                "positions_file",
                "Parent directory for does not exists for {:?}",
                self.positions_file
            );
        };

        if self.ssl.is_some() {
            self.ssl.as_ref().unwrap().check()?
        }

        self.transcoding.check()?;
        self.thread_pool.check()?;
        self.chapters.check()?;

        if self.base_dirs.is_empty() {
            return value_error!(
                "base_dirs",
                "At least one directory with audio files must be provided"
            );
        }

        for d in &self.base_dirs {
            if !d.is_dir() {
                return value_error!("base_dir", "{:?} is not direcrory", d);
            }
        }

        if let Some(url) = &self.url_path_prefix {
            if let Err(e) = validators::is_valid_url_path_prefix(url.clone()) {
                return value_error!("url_path_prefix", e);
            }
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        let data_base_dir = base_data_dir();
        Config {
            base_dirs: vec![],
            url_path_prefix: None,
            listen: ([0, 0, 0, 0], 3000u16).into(),
            thread_pool: ThreadPoolConfig::default(),
            shared_secret: None,
            limit_rate: None,
            transcoding: TranscodingConfig::default(),
            token_validity_hours: 365 * 24,
            client_dir: "client/dist".into(),
            secret_file: data_base_dir.join("audioserve.secret"),
            cors: false,
            ssl: None,
            allow_symlinks: false,
            search_cache: false,
            disable_folder_download: false,
            chapters: ChaptersSize::default(),
            no_dir_collaps: false,
            ignore_chapters_meta: false,
            positions_file: data_base_dir.join("audioserve.positions"),
            positions_ws_timeout: Duration::from_secs(600),
            behind_proxy: false,
        }
    }
}

pub fn init_config() -> Result<()> {
    unsafe {
        if CONFIG.is_some() {
            panic!("Config is already initialied")
        }

        BASE_DATA_DIR = Some(dirs::home_dir().unwrap_or_default().join(".audioserve"));
    }

    let config = cli::parse_args()?;

    unsafe {
        CONFIG = Some(config);
    }

    Ok(())
}

#[cfg(test)]
pub mod init {
    /// Static config initialization for tests
    /// as tests are run concurrently it requires also some synchronication
    use super::{Config, BASE_DATA_DIR, CONFIG};
    use std::path::PathBuf;
    use std::sync::Once;
    static INIT: Once = Once::new();
    /// this default config is used only for testing
    pub fn init_default_config() {
        INIT.call_once(|| {
            let base_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            unsafe {
                BASE_DATA_DIR = Some(base_dir);
            }
            let config = Config::default();
            unsafe {
                CONFIG = Some(config);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::init::init_default_config;
    use super::*;
    #[test]
    fn test_default_serialize() {
        init_default_config();
        let config = Config::default();
        let s = serde_yaml::to_string(&config).unwrap();
        assert!(s.len() > 100);
        println!("{}", s);

        let des: Config = serde_yaml::from_str(&s).unwrap();
        assert_eq!(config.transcoding.medium, des.transcoding.medium);
    }

    use crate::services::transcode::QualityLevel;
    #[test]
    fn test_transcoding_profile_deserialize() {
        fn load_file(fname: &str) -> Config {
            let f = File::open(fname).unwrap();
            serde_yaml::from_reader(f).unwrap()
        }
        init_default_config();
        let c1 = load_file("./test_data/transcodings.yaml");
        assert_eq!(c1.transcoding.get(QualityLevel::Medium).bitrate(), 24);
        let c2 = load_file("./test_data/transcodings.1.yaml");
        assert_eq!(
            c2.transcoding.get(QualityLevel::Medium).format_name(),
            "mp3"
        );
        let c3 = load_file("./test_data/transcodings.2.yaml");
        assert_eq!(c3.transcoding.get(QualityLevel::High).bitrate(), 96);
    }
}
