use simple_file_cache::Cache;
use std::fs;
use std::env;
use std::path::{Path,PathBuf};
use crate::services::transcode::QualityLevel;


lazy_static! {
    pub static ref CACHE:Cache = {
        let cache_dir = env::temp_dir().join("audioserve-cache");
        if ! cache_dir.exists() {
            fs::create_dir(&cache_dir).expect("Cannot create directory for cache")
        }
        Cache::new(cache_dir, 2*1024*1024*1024, 1024).expect("Cannot create cache")
    };
}

//TODO: not ideal as potential collisions for non-unicode names
pub fn cache_key<P: AsRef<Path>>(file:P, quality: &QualityLevel) -> String {
 let key = PathBuf::from(quality.to_letter()).join(file.as_ref());
 key.to_string_lossy().into()
}

pub fn get_cache() -> &'static Cache {
    &CACHE
}




//TODO : Chunk caching stream? Cache sink? How to find that trancoding was finished succesfully??

