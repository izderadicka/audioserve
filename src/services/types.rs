use super::transcode::{QualityLevel, TranscodingFormat};
use crate::config::get_config;
use collection::{AudioFile, AudioFolderShort};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CollectionsInfo {
    pub version: &'static str,
    pub folder_download: bool,
    pub shared_positions: bool,
    pub count: u32,
    pub names: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct TranscodingSummary {
    bitrate: u32,
    name: &'static str,
}

impl From<TranscodingFormat> for TranscodingSummary {
    fn from(f: TranscodingFormat) -> Self {
        TranscodingSummary {
            bitrate: f.bitrate(),
            name: f.format_name(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Transcodings {
    pub max_transcodings: usize,
    pub low: TranscodingSummary,
    pub medium: TranscodingSummary,
    pub high: TranscodingSummary,
}

impl Default for Transcodings {
    fn default() -> Self {
        Transcodings::new()
    }
}

impl Transcodings {
    pub fn new() -> Self {
        let cfg = get_config();
        Transcodings {
            max_transcodings: cfg.transcoding.max_parallel_processes,
            low: cfg.transcoding.get(QualityLevel::Low).into(),
            medium: cfg.transcoding.get(QualityLevel::Medium).into(),
            high: cfg.transcoding.get(QualityLevel::High).into(),
        }
    }

    pub fn for_user_agent(user_agent: &str) -> Self {
        let alt_configs = get_config().transcoding.alt_configs();
        if let Some(alt_configs) = alt_configs {
            for (re, cfg) in alt_configs {
                if re.is_match(user_agent) {
                    debug!(
                        "Using alternate transcoding {} config for User Agent {} ",
                        re, user_agent
                    );
                    return Transcodings {
                        max_transcodings: get_config().transcoding.max_parallel_processes,
                        low: cfg.get(QualityLevel::Low).into(),
                        medium: cfg.get(QualityLevel::Medium).into(),
                        high: cfg.get(QualityLevel::High).into(),
                    };
                }
            }
        }
        Transcodings::new()
    }
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub files: Vec<AudioFile>,
    pub subfolders: Vec<AudioFolderShort>,
}

#[cfg(feature = "folder-download")]
pub use download_format::DownloadFormat;
use serde::Serialize;

#[cfg(feature = "folder-download")]
mod download_format {

    use crate::error::Error;
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DownloadFormat {
        Tar,
        Zip,
    }

    impl DownloadFormat {
        pub fn extension(&self) -> &'static str {
            match self {
                DownloadFormat::Tar => ".tar",
                DownloadFormat::Zip => ".zip",
            }
        }

        pub fn mime(&self) -> mime::Mime {
            match self {
                DownloadFormat::Tar => "application/x-tar".parse().unwrap(),
                DownloadFormat::Zip => "application/zip".parse().unwrap(),
            }
        }
    }

    impl std::str::FromStr for DownloadFormat {
        type Err = Error;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "tar" => Ok(DownloadFormat::Tar),
                "zip" => Ok(DownloadFormat::Zip),
                _ => Err(Error::msg("Invalid download archive format tag")),
            }
        }
    }

    impl Default for DownloadFormat {
        fn default() -> Self {
            if cfg!(feature = "folder-download-default-tar") {
                DownloadFormat::Tar
            } else {
                DownloadFormat::Zip
            }
        }
    }
}
