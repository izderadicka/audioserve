use crate::config::get_config;
use simple_file_cache::Cache;
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

lazy_static! {
    pub static ref CACHE: Option<Cache> = {
        let cfg = get_config();
        if cfg.icons.cache_disabled {
            None
        } else {
            let cache_dir = &cfg.icons.cache_dir;
            if !cache_dir.exists() {
                fs::create_dir(&cache_dir).expect("Cannot create directory for icons cache")
            }
            Some(
                Cache::new(
                    cache_dir,
                    u64::from(cfg.icons.cache_max_size) * 1024 * 1024,
                    cfg.icons.cache_max_files.into(),
                )
                .expect("Cannot create cache"),
            )
        }
    };
}

pub fn cached_icon(file: impl AsRef<Path>) -> Option<File> {
    let key = cache_key(&file);
    get_cache()
        .get(key.as_ref())
        .transpose()
        .unwrap_or_else(|e| {
            error!("Icons cache error: {}", e);
            None
        })
}

pub fn cache_icon(file: impl AsRef<Path>, data: impl AsRef<[u8]>) -> anyhow::Result<()> {
    let key = cache_key(&file);
    let mut f = get_cache().add(key)?;
    f.write_all(data.as_ref())?;
    f.finish()?;
    Ok(())
}

//TODO: not ideal as potential collisions for non-unicode names
pub fn cache_key<P: AsRef<Path>>(file: &P) -> Cow<'_, str> {
    file.as_ref().to_string_lossy()
}

pub fn get_cache() -> &'static Cache {
    CACHE.as_ref().unwrap()
}
