use collection::MINIMUM_CHAPTER_DURATION;
use regex::Regex;
use serde::{Deserialize, Serialize};

pub use self::error::{Error, Result};
use super::services::transcode::{QualityLevel, TranscodingFormat};
use crate::services::transcode::codecs::{Bandwidth, Opus};
use crate::util;
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::env;
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

mod cli;
mod validators;
#[macro_use]
mod error;

static mut CONFIG: Option<Config> = None;

pub const LONG_VERSION: &str = env!("AUDIOSERVE_LONG_VERSION");
pub const FEATURES: &str = env!("AUDIOSERVE_FEATURES");
const CD_FOLDER_RE: &str = r"^CD[ -_]?\s*\d+\s*$";

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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct IconsConfig {
    pub cache_dir: PathBuf,
    pub cache_max_size: u32,
    pub cache_max_files: u32,
    pub cache_disabled: bool,
    pub size: u32,
    pub cache_save_often: bool,
    pub fast_scaling: bool,
}

impl Default for IconsConfig {
    fn default() -> Self {
        let data_base_dir = base_data_dir();
        let cache_dir = data_base_dir.join("icons-cache");
        IconsConfig {
            cache_dir,
            cache_max_size: 100,
            cache_max_files: 1024,
            cache_disabled: false,
            cache_save_often: false,
            size: 128,
            fast_scaling: false,
        }
    }
}

impl IconsConfig {
    pub fn check(&self) -> Result<()> {
        if !util::parent_dir_exists(&self.cache_dir) {
            return value_error!(
                "icons.cache_root_dir",
                "Parent directory does not exists for {:?}",
                self.cache_dir
            );
        };

        if self.cache_max_size < 10 {
            return value_error!(
                "max_size",
                "Icons cache small then 10 MB does not make sense"
            );
        }

        if self.cache_max_files < 100 {
            return value_error!(
                "max_size",
                "Icons cache with less the 100 files does not make sense"
            );
        }

        Ok(())
    }
}

#[cfg(feature = "transcoding-cache")]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct TranscodingConfig {
    pub max_parallel_processes: usize,
    pub max_runtime_hours: u32,
    #[cfg(feature = "transcoding-cache")]
    pub cache: TranscodingCacheConfig,
    low: TranscodingFormat,
    medium: TranscodingFormat,
    high: TranscodingFormat,
    alt_configs: Option<HashMap<String, TranscodingDetails>>,
    #[serde(skip)]
    alt_configs_inner: Option<Vec<(regex::Regex, TranscodingDetails)>>,
}

impl Default for TranscodingConfig {
    fn default() -> Self {
        TranscodingConfig {
            max_parallel_processes: (2 * num_cpus::get()).max(4),
            max_runtime_hours: 24,
            #[cfg(feature = "transcoding-cache")]
            cache: TranscodingCacheConfig::default(),
            low: TranscodingFormat::OpusInOgg(Opus::new(32, 5, Bandwidth::SuperWideBand, true)),
            medium: TranscodingFormat::OpusInOgg(Opus::new(48, 8, Bandwidth::SuperWideBand, false)),
            high: TranscodingFormat::OpusInOgg(Opus::new(64, 10, Bandwidth::FullBand, false)),
            alt_configs: None,
            alt_configs_inner: None,
        }
    }
}

macro_rules! implement_get_transcoding {
    ($($trans_config:ty),*) => {
        $(
        impl $trans_config {
            pub fn get(&self, quality: QualityLevel) -> TranscodingFormat {
                match quality {
                    QualityLevel::Low => self.low.clone(),
                    QualityLevel::Medium => self.medium.clone(),
                    QualityLevel::High => self.high.clone(),
                    QualityLevel::Passthrough => TranscodingFormat::Remux,
                }
            }
        }
        )*

    };
}

impl TranscodingConfig {
    pub fn check(&self) -> Result<()> {
        if self.max_parallel_processes < 4 {
            return value_error!(
                "max_parallel_processes",
                "With less then 4 transcoding processes audioserve will not work properly"
            );
        } else if self.max_parallel_processes > 200 {
            return value_error!(
                "max_parallel_processes",
                "As transcodings are resource intesive, having more then 200 is not wise"
            );
        }

        if self.max_runtime_hours < 1 {
            return value_error!("max_runtime_hours", "Minimum time is 1 hour");
        }

        if let Some(alt_configs) = self.alt_configs.as_ref() {
            for re in alt_configs.keys() {
                regex::Regex::new(re)
                    .map(|_re| ())
                    .or_else(|e| value_error!("alt_encodings", "Invalid User Agent regex {}", e))?
            }
        }
        #[cfg(feature = "transcoding-cache")]
        self.cache.check()?;
        Ok(())
    }

    pub fn prepare(&mut self) -> Result<()> {
        if let Some(alt_configs) = self.alt_configs.take() {
            if !alt_configs.is_empty() {
                self.alt_configs_inner = Some(
                    alt_configs
                        .into_iter()
                        // I can unwrap because regex we checked by check fh
                        .map(|(re, mut cfg)| {
                            cfg.tag = generate_tag(&re);
                            (regex::Regex::new(&re), cfg)
                        })
                        .map(|(res, cfg)| match res {
                            Ok(re) => Ok((re, cfg)),
                            Err(e) => Err(Error::in_value(
                                "alt_configs",
                                format!("Invalid regex {}", e),
                            )),
                        })
                        .collect::<Result<Vec<_>>>()?,
                )
            }
        }
        Ok(())
    }

    pub fn alt_configs(&self) -> Option<&Vec<(regex::Regex, TranscodingDetails)>> {
        self.alt_configs_inner.as_ref()
    }
}

fn generate_tag(s: &str) -> String {
    let hash = ring::digest::digest(&ring::digest::SHA256, s.as_bytes());
    format!(
        "{:016x}",
        u64::from_be_bytes(hash.as_ref()[..8].try_into().expect("Invalid size"))
    )
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TranscodingDetails {
    #[serde(skip)]
    pub tag: String,
    low: TranscodingFormat,
    medium: TranscodingFormat,
    high: TranscodingFormat,
}

implement_get_transcoding!(TranscodingConfig, TranscodingDetails);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct CollectionConfig {
    pub dont_watch_for_changes: bool,
    pub changes_debounce_interval: u32,
}

impl Default for CollectionConfig {
    fn default() -> Self {
        Self {
            dont_watch_for_changes: false,
            changes_debounce_interval: 10,
        }
    }
}

impl CollectionConfig {
    fn check(&self) -> Result<()> {
        if self.changes_debounce_interval < 1 {
            return value_error!("changes_debounce_interval", "Must be bigger then 0");
        } else if self.changes_debounce_interval > 1800 {
            return value_error!(
                "changes_debounce_interval",
                "Interval is too big, this cause performance problems"
            );
        }

        Ok(())
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
#[serde(deny_unknown_fields)]
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
        if self.from_duration > 0 && self.from_duration < MINIMUM_CHAPTER_DURATION {
            return value_error!(
                "from_duration",
                "File shorter then 10 mins should not be split to chapters"
            );
        }

        if self.duration < MINIMUM_CHAPTER_DURATION {
            return value_error!("duration", "Minimal chapter duration is 10 minutes");
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SslConfig {
    pub key_file: PathBuf,
    pub cert_file: PathBuf,
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
pub enum PositionsBackupFormat {
    None,
    Legacy,
    V1,
}

impl FromStr for PositionsBackupFormat {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "legacy" => Ok(PositionsBackupFormat::Legacy),
            "v1" => Ok(PositionsBackupFormat::V1),
            _ => value_error!("positions-restore", "Invalid version"),
        }
    }
}

#[cfg(feature = "shared-positions")]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct PositionsConfig {
    pub ws_timeout: Duration,
    pub backup_file: Option<PathBuf>,
    pub restore: PositionsBackupFormat,
    pub backup_schedule: Option<String>,
}

#[cfg(feature = "shared-positions")]
impl Default for PositionsConfig {
    fn default() -> Self {
        Self {
            ws_timeout: Duration::from_secs(600),
            backup_file: None,
            restore: PositionsBackupFormat::None,
            backup_schedule: None,
        }
    }
}

#[cfg(feature = "shared-positions")]
impl PositionsConfig {
    pub fn check(&self) -> Result<()> {
        if self.ws_timeout < Duration::from_secs(60) {
            return value_error!("positions-ws-timeout", "Timeout must be at least 60s");
        }

        if let Some(schedule) = self.backup_schedule.as_ref() {
            if crate::util::parse_cron(schedule).is_err() {
                return value_error!("positions-backup-schedule", "Invalid cron expression");
            }
        }
        Ok(())
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorsConfig {
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(skip)]
    pub allow: Cors,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollapseCDFolderConfig {
    #[serde(default)]
    pub regex: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Cors {
    AllowAllOrigins,
    AllowMatchingOrigins(Regex),
}

impl Default for Cors {
    fn default() -> Self {
        Cors::AllowAllOrigins
    }
}

impl FromStr for Cors {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let re = Regex::new(s)
            .map_err(|e| Error::in_value("cors-regex", format!("Invalid cors regex: {}", e)))?;
        Ok(Cors::AllowMatchingOrigins(re))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub thread_pool: ThreadPoolConfig,
    pub base_dirs: Vec<PathBuf>,
    pub base_dirs_options: HashMap<PathBuf, String>,
    pub url_path_prefix: Option<String>,
    pub shared_secret: Option<String>,
    pub limit_rate: Option<f32>,
    #[serde(with = "serde_yaml::with::singleton_map_recursive")]
    // to keep backward compatibility with existing configs
    pub transcoding: TranscodingConfig,
    pub token_validity_hours: u32,
    pub secret_file: PathBuf,
    pub client_dir: PathBuf,
    pub cors: Option<CorsConfig>,
    pub ssl: Option<SslConfig>,
    pub allow_symlinks: bool,
    pub search_cache: bool,
    pub disable_folder_download: bool,
    pub chapters: ChaptersSize,
    pub no_dir_collaps: bool,
    pub ignore_chapters_meta: bool,
    #[cfg(feature = "shared-positions")]
    pub positions: PositionsConfig,
    pub behind_proxy: bool,
    pub collections_cache_dir: PathBuf,
    pub tags: HashSet<String>,
    pub force_cache_update_on_init: bool,
    pub natural_files_ordering: bool,
    pub static_resource_cache_age: Option<u32>,
    pub folder_file_cache_age: Option<u32>,
    pub collapse_cd_folders: Option<CollapseCDFolderConfig>,
    #[cfg(feature = "tags-encoding")]
    pub tags_encoding: Option<String>,
    pub icons: IconsConfig,
    pub time_to_folder_end: u32,
    pub read_playlist: bool,
    pub collections_options: CollectionConfig,
    pub compress_responses: bool,
}

impl Config {
    pub fn add_base_dir<P: AsRef<str>>(&mut self, p: P) -> Result<()> {
        let mut parts = p.as_ref().splitn(2, ':');
        let base_dir = parts
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| Error::in_value("base_dir", "Empty base_dir, nothing before :"))?;

        if !base_dir.is_dir() {
            return value_error!("base_dir", "{:?} is not direcrory", base_dir);
        }

        if let Some(options) = parts.next() {
            self.base_dirs_options
                .insert(base_dir.clone(), options.to_string());
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
    /// Any runtime optimalizations, compilatipons of config
    pub fn prepare(&mut self) -> Result<()> {
        self.transcoding.prepare()?;

        if let Some(ref mut cors) = self.cors {
            if let Some(ref re) = cors.regex {
                cors.allow = re.parse()?;
            }
        }

        if let Some(ref mut collapse) = self.collapse_cd_folders {
            if collapse.regex.is_none() {
                collapse.regex = Some(CD_FOLDER_RE.into());
            }
        }
        Ok(())
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

        if self.ssl.is_some() {
            self.ssl.as_ref().unwrap().check()?
        }

        self.transcoding.check()?;
        self.icons.check()?;
        self.thread_pool.check()?;
        self.chapters.check()?;
        #[cfg(feature = "shared-positions")]
        self.positions.check()?;
        self.collections_options.check()?;

        if self.base_dirs.is_empty() {
            return value_error!(
                "base_dirs",
                "At least one directory with audio files must be provided"
            );
        }

        if self.base_dirs.len() > 100 {
            return value_error!("base_dirs", "Too many collections directories (max is 100)");
        }

        for d in &self.base_dirs {
            if !d.is_dir() {
                return value_error!("base_dir", "{:?} is not direcrory", d);
            }
        }

        if let Some(url) = &self.url_path_prefix {
            if let Err(e) = validators::is_valid_url_path_prefix(url.as_str()) {
                return value_error!("url_path_prefix", e.to_string());
            }
        }

        if let Some(ref c) = self.collapse_cd_folders {
            if let Some(ref re) = c.regex {
                Regex::new(re)
                    .or_else(|e| value_error!("cd-folder-regex", "Invalid regex: {}", e))?;
            }
        }

        Ok(())
    }

    pub fn get_tags(&self) -> Option<HashSet<String>> {
        if self.tags.is_empty() {
            None
        } else {
            Some(self.tags.clone())
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let data_base_dir = base_data_dir();
        Config {
            base_dirs: vec![],
            base_dirs_options: HashMap::new(),
            url_path_prefix: None,
            listen: ([0, 0, 0, 0], 3000u16).into(),
            thread_pool: ThreadPoolConfig::default(),
            shared_secret: None,
            limit_rate: None,
            transcoding: TranscodingConfig::default(),
            token_validity_hours: 365 * 24,
            #[cfg(test)]
            client_dir: "test_data".into(),
            #[cfg(not(test))]
            client_dir: "client/dist".into(),
            secret_file: data_base_dir.join("audioserve.secret"),
            cors: None,
            ssl: None,
            allow_symlinks: false,
            search_cache: false,
            disable_folder_download: false,
            chapters: ChaptersSize::default(),
            no_dir_collaps: false,
            ignore_chapters_meta: false,
            behind_proxy: false,
            collections_cache_dir: data_base_dir.join("col_db"),
            tags: HashSet::new(),
            force_cache_update_on_init: false,
            natural_files_ordering: true,
            #[cfg(feature = "shared-positions")]
            positions: Default::default(),
            static_resource_cache_age: None,
            folder_file_cache_age: Some(24 * 3600),
            collapse_cd_folders: None,
            #[cfg(feature = "tags-encoding")]
            tags_encoding: None,
            icons: IconsConfig::default(),
            time_to_folder_end: 10,
            read_playlist: false,
            collections_options: CollectionConfig::default(),
            compress_responses: false,
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
