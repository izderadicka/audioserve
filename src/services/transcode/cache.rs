use crate::config::get_config;
use crate::services::transcode::TimeSpan;
use simple_file_cache::AsyncCache as Cache;
use std::fs;
use std::path::Path;

use super::ChosenTranscoding;

lazy_static! {
    pub static ref CACHE: Option<Cache> = {
        let cfg = get_config();
        if cfg.transcoding.cache.disabled {
            None
        } else {
            let cache_dir = &cfg.transcoding.cache.root_dir;
            if !cache_dir.exists() {
                fs::create_dir(&cache_dir).expect("Cannot create directory for cache")
            }
            Some(
                Cache::new(
                    cache_dir,
                    u64::from(cfg.transcoding.cache.max_size) * 1024 * 1024,
                    cfg.transcoding.cache.max_files.into(),
                )
                .expect("Cannot create cache"),
            )
        }
    };
}

//TODO: not ideal as potential collisions for non-unicode names
pub fn cache_key<P: AsRef<Path>>(
    file: P,
    quality: &ChosenTranscoding,
    span: Option<TimeSpan>,
) -> String {
    let mut key: String = quality.level.to_letter().into();
    if !quality.tag.is_empty() {
        key.push_str(quality.tag);
    }
    key.push_str(&file.as_ref().to_string_lossy());

    if let Some(span) = span {
        key.push('/');
        key.push_str(&span.to_string());
    }
    key
}

pub fn get_cache() -> &'static Cache {
    CACHE.as_ref().unwrap()
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::services::transcode::{QualityLevel, TranscodingFormat};

    #[test]
    fn test_cache_key() {
        let key = cache_key(
            "/home/ivan/neco",
            &ChosenTranscoding {
                level: QualityLevel::Medium,
                format: TranscodingFormat::Remux,
                tag: "abcd",
            },
            Some(TimeSpan {
                start: 0,
                duration: Some(5),
            }),
        );
        assert_eq!("mabcd/home/ivan/neco/0-5", key);
    }
}
