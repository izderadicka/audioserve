use crate::config::get_config;
use crate::services::transcode::{QualityLevel, TimeSpan};
use simple_file_cache::Cache;
use std::fs;
use std::path::{Path};


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
pub fn cache_key<P: AsRef<Path>>(file: P, quality: QualityLevel, span: Option<TimeSpan>) -> String {
    
    let mut key: String = quality.to_letter().into();
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

    #[test]
    fn test_cache_key() {
        let key = cache_key("/home/ivan/neco", QualityLevel::Medium, 
        Some(TimeSpan{start:0, duration:Some(5)}));
        assert_eq!("m/home/ivan/neco/0-5", key);
    }
}
