use crate::config::get_config;
use crate::services::transcode::QualityLevel;
use simple_file_cache::Cache;
use std::fs;
use std::path::{Path, PathBuf};

lazy_static! {
    pub static ref CACHE: Option<Cache> = {
        let cfg = get_config();
        if cfg.transcoding_cache.disabled {
            None
        } else {
            let cache_dir = &cfg.transcoding_cache.root_dir;
            if !cache_dir.exists() {
                fs::create_dir(&cache_dir).expect("Cannot create directory for cache")
            }
            Some(
                Cache::new(
                    cache_dir,
                    cfg.transcoding_cache.max_size,
                    cfg.transcoding_cache.max_files,
                )
                .expect("Cannot create cache"),
            )
        }
    };
}

//TODO: not ideal as potential collisions for non-unicode names
pub fn cache_key<P: AsRef<Path>>(file: P, quality: &QualityLevel) -> String {
    let key = PathBuf::from(quality.to_letter()).join(file.as_ref());
    key.to_string_lossy().into()
}

pub fn get_cache() -> &'static Cache {
    CACHE.as_ref().unwrap()
}

//TODO : Chunk caching stream? Cache sink? How to find that trancoding was finished succesfully??
