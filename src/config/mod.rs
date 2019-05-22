use super::services::transcode::{QualityLevel, Transcoder, TranscodingFormat};

use num_cpus;
use serde_yaml;
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
        let root_dir = env::temp_dir().join("audioserve-cache");
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
        if let Some(true) = self.root_dir.parent().map(Path::is_dir) {
           
        } else {
            return value_error!("root_dir", "Parent directory does not exists for {:?}", self.root_dir)
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
    pub max_parallel_processes: u32,
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
            max_parallel_processes: (2 * num_cpus::get()) as u32,
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
            return value_error!("max_parallel_processes", "With less then 2 transcoding processes audioserve will not work properly")
        } else if self.max_parallel_processes > 100 {
             return value_error!("max_parallel_processes", "As transcodings are resource intesive, having more then 100 is not wise")
        }
       
        if self.max_runtime_hours <1 {
            return value_error!("max_runtime_hours", "Minimum time is 1 hour")
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
    pub fn check(& self) -> Result<()> {
        if self.from_duration > 0 && self.from_duration < 10 {
            return value_error!("from_duration", "File shorter then 10 mins should not be split to chapters")
        }

        if self.duration < 10 {
            return value_error!("duration", "Minimal chapter duration is 10 minutes")
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
        if self.shared_secret.as_ref().map(String::len).unwrap_or(0) < 3 {
            return value_error!("shared_secret", "Shared secret must be at least 3 bytes");
        }

        if self.token_validity_hours < 240 {
            return value_error!(
                "token-validity-days",
                "Token must be valid for at least 10 days"
            );
        }
        
        if !self.client_dir.is_dir() {
            return value_error!(
                "client_dir",
                "Directory with web client files {:?} does not exists or is not directory",
                self.client_dir
            );
        }
       
        if let Some(true) = self.secret_file.parent().map(Path::is_dir) {
           
        } else {
            return value_error!(
                "secret_file",
                "Parent directory for does not exists for {:?}",
                self.secret_file
            )
        };
    
        if let Some(true) = self.positions_file.parent().map(Path::is_dir) {
            
        } else {
            return value_error!(
                "positions_file",
                "Parent directory for does not exists for {:?}",
                self.positions_file
            )
        };

        if self.ssl.is_some() {
            self.ssl.as_ref().unwrap().check()?
        }

        self.transcoding.check()?;
        self.thread_pool.check()?;
        self.chapters.check()?;

        Ok(())
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
    warn!("START reading config");
    let config = Config::default();
    let config = cli::parse_args(config)?;
    config.check()?;

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
    fn test_default_serialize() {
        let config = Config::default();
        let s = serde_yaml::to_string(&config).unwrap();
        assert!(s.len() > 100);
        println!("{}", s);

        let des: Config = serde_yaml::from_str(&s).unwrap();
        assert_eq!(config.transcoding.medium, des.transcoding.medium);
    }

    use crate::services::transcode::QualityLevel;
    #[test]
    fn test_transcoding_profile_deserialize()  {
        fn load_file(fname: &str) -> Config{
            let f = File::open(fname).unwrap();
            serde_yaml::from_reader(f).unwrap()
        }
        let c1 = load_file("./test_data/transcodings.yaml");
        assert_eq!(c1.transcoding.get(QualityLevel::Medium).bitrate(), 24);
        let c2 = load_file("./test_data/transcodings.1.yaml");
        assert_eq!(c2.transcoding.get(QualityLevel::Medium).format_name(), "mp3");
        let c3 = load_file("./test_data/transcodings.2.yaml");
        assert_eq!(c3.transcoding.get(QualityLevel::High).bitrate(), 96);
    }

}
