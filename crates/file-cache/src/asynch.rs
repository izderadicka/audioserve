use super::{error::Error, CacheInner};
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tokio::task::spawn_blocking;

impl From<tokio::task::JoinError> for Error {
    fn from(_f: tokio::task::JoinError) -> Self {
        Error::Executor
    }
}

fn invert<T>(x: Option<Result<T>>) -> Result<Option<T>> {
    x.map_or(Ok(None), |v| v.map(Some))
}

type Result<T> = std::result::Result<T, Error>;
type CacheInnerType = Arc<RwLock<CacheInner>>;

#[derive(Clone)]
pub struct Cache {
    inner: CacheInnerType,
}

impl Cache {
    pub fn new<P: AsRef<Path>>(root: P, max_size: u64, max_files: u64) -> Result<Self> {
        let root = root.as_ref().into();
        CacheInner::new(root, max_size, max_files).map(|cache| Cache {
            inner: Arc::new(RwLock::new(cache)),
        })
    }

    /// return tuple (free_files, free_size)
    pub fn free_capacity(&self) -> (u64, u64) {
        let c = self.inner.read().unwrap();
        (c.max_files - c.num_files, c.max_size - c.size)
    }

    pub async fn add<S: AsRef<str>>(
        &self,
        key: S,
        mtime: SystemTime,
    ) -> Result<(tokio::fs::File, Finisher)> {
        let cache = self.inner.clone();
        let key = key.as_ref().to_string();
        spawn_blocking(move || {
            let mut c = cache.write().expect("Cannot lock cache");
            c.add(key.clone(), mtime)
                .and_then(|f| f.try_clone().map_err(|e| e.into()).map(|f2| (f, f2)))
                .map(|(f, f2)| {
                    (
                        tokio::fs::File::from_std(f),
                        Finisher {
                            cache: cache.clone(),
                            key,
                            file: f2,
                        },
                    )
                })
        })
        .await?
    }

    pub async fn get<S: AsRef<str>>(
        &self,
        key: S,
        mtime: SystemTime,
    ) -> Result<Option<tokio::fs::File>> {
        let key = key.as_ref().to_string();
        let inner = self.inner.clone();
        let r = spawn_blocking(move || {
            let mut c = inner.write().expect("Cannot lock cache");
            c.get(key, mtime).map(|f| f.map(tokio::fs::File::from_std))
        })
        .await?;
        invert(r)
    }

    pub async fn get2<S: AsRef<str>>(
        &self,
        key: S,
        mtime: SystemTime,
    ) -> Result<Option<(tokio::fs::File, std::path::PathBuf)>> {
        let cache = self.inner.clone();
        let key = key.as_ref().to_string();
        let r = spawn_blocking(move || {
            let mut c = cache.write().expect("Cannot lock cache");
            c.get2(key, mtime)
                .map(|f| f.map(|(f, path)| (tokio::fs::File::from_std(f), path)))
        })
        .await?;
        invert(r)
    }

    pub async fn save_index(&self) -> Result<()> {
        let cache = self.inner.clone();
        spawn_blocking(move || {
            let cache = cache.write().unwrap();
            cache.save_index()
        })
        .await?
    }

    pub fn save_index_blocking(&self) -> Result<()> {
        let cache = self.inner.write().expect("Cannot lock cache");
        cache.save_index()
    }
}

pub struct Finisher {
    pub(crate) cache: CacheInnerType,
    pub(crate) key: String,
    pub(crate) file: fs::File,
}

impl Finisher {
    pub async fn commit(mut self) -> Result<()> {
        spawn_blocking(move || {
            let mut c = self.cache.write().expect("Cannot lock cache");
            c.finish(self.key, &mut self.file)
        })
        .await?
    }

    pub async fn roll_back(self) -> Result<()> {
        spawn_blocking(move || super::cleanup(&self.cache, self.key))
            .await
            .map_err(From::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    const MY_KEY: &str = "muj_test_1";
    const MSG: &str = "Hello there you lonely bastard";

    async fn cache_rw(c: Cache, t: SystemTime) -> Result<()> {
        let (mut f, fin) = c.add(MY_KEY, t).await?;
        f.write_all(MSG.as_bytes()).await?;
        fin.commit().await?;
        match c.get(MY_KEY, SystemTime::now()).await? {
            None => panic!("cache file not found"),
            Some(mut f) => {
                let mut v = Vec::new();
                f.read_to_end(&mut v).await?;
                let s = std::str::from_utf8(&v).unwrap();
                assert_eq!(MSG, s);
                info!("ALL DONE");
            }
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_async() {
        env_logger::try_init().ok();
        let temp_dir = tempdir().unwrap();
        let c = Cache::new(temp_dir.path(), 10000, 10).unwrap();
        let c2 = c.clone();
        let t = SystemTime::now();
        cache_rw(c, t).await.unwrap();
        let mut f = c2.get(MY_KEY, t).await.unwrap().unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).await.unwrap();
        assert_eq!(MSG, s);
        ()
    }
}
