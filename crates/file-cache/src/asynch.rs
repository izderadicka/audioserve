use super::error::Error;
use super::CacheInner;
use std::fs;
use std::sync::{Arc, RwLock};
use tokio::prelude::*;
use tokio_threadpool::blocking;
use tokio::fs as tokio_fs;

pub struct CacheFileWrite {
    cache: Arc<RwLock<CacheInner>>,
    key: Option<String>
}

pub struct CacheFileRead<S> {
    cache: Arc<RwLock<CacheInner>>,
    key: Option<S>
}

impl <S: AsRef<str>> CacheFileRead<S> {
    pub(crate) fn new(cache: Arc<RwLock<CacheInner>>, key:S) -> Self {
        CacheFileRead {
            cache,
            key: Some(key)
        }
    }
}

impl <S: AsRef<str>+Clone> Future for CacheFileRead<S> {
    type Error = Error;
    type Item = Option<tokio_fs::File>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.key.take() {
            None => panic!("Calling resolved future"),
            Some(key) => match blocking(|| {
                let mut c = self.cache.write().expect("Cannot lock cache");
                c.get(key.clone()).map(|f| f.map(|f| tokio_fs::File::from_std(f)))
                }) {
                Err(e) => Err(e.into()),
                Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
                Ok(Async::Ready(Some(Err(e)))) => Err(e.into()),
                Ok(Async::Ready(Some(Ok(r)))) => Ok(Async::Ready(Some(r))),
                Ok(Async::NotReady) => {
                    self.key = Some(key);
                    Ok(Async::NotReady)
                    },
            },
        }
    }
}

pub struct CacheFileRead2<S> {
    cache: Arc<RwLock<CacheInner>>,
    key: Option<S>
}

impl <S: AsRef<str>> CacheFileRead2<S> {
    pub(crate) fn new(cache: Arc<RwLock<CacheInner>>, key:S) -> Self {
        CacheFileRead2 {
            cache,
            key: Some(key)
        }
    }
}

impl <S: AsRef<str>+Clone> Future for CacheFileRead2<S> {
    type Error = Error;
    type Item = Option<(tokio_fs::File, std::path::PathBuf)>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.key.take() {
            None => panic!("Calling resolved future"),
            Some(key) => match blocking(|| {
                let mut c = self.cache.write().expect("Cannot lock cache");
                c.get2(key.clone()).map(|f| f.map(|(f,path)| (tokio_fs::File::from_std(f),path)))
                }) {
                Err(e) => Err(e.into()),
                Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
                Ok(Async::Ready(Some(Err(e)))) => Err(e.into()),
                Ok(Async::Ready(Some(Ok(r)))) => Ok(Async::Ready(Some(r))),
                Ok(Async::NotReady) => {
                    self.key = Some(key);
                    Ok(Async::NotReady)
                    },
            },
        }
    }
}

impl Future for CacheFileWrite {
    type Item = (tokio_fs::File, Finisher);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.key.take() {
            None => panic!("Calling resolved future"),
            Some(key) => match blocking(|| {
                let mut c = self.cache.write().expect("Cannot lock cache");
                c.add(key.clone())
                    .and_then(|f| f.try_clone().map_err(|e| e.into()).map(|f2| (f, f2)))
                    .map(|(f, f2)| {
                        (
                            tokio_fs::File::from_std(f),
                            Finisher {
                                cache: self.cache.clone(),
                                key: Some(key.clone()),
                                file: f2,
                            },
                        )
                    })
            }) {
                Err(e) => Err(e.into()),
                Ok(Async::Ready(Err(e))) => Err(e.into()),
                Ok(Async::Ready(Ok(r))) => Ok(Async::Ready(r)),
                Ok(Async::NotReady) => {
                    self.key = Some(key);
                    Ok(Async::NotReady)
                    },
            },
        }
    }
}

impl CacheFileWrite {
   pub (crate) fn new(cache: Arc<RwLock<CacheInner>>,
        key: String) -> Self {
            CacheFileWrite {
                cache,
                key: Some(key)
            }
        } 
}

pub struct Finisher{
    pub(crate) cache: Arc<RwLock<CacheInner>>,
    pub(crate) key: Option<String>,
    pub(crate) file: fs::File,
}

impl Finisher {
    pub fn commit(self) -> Finish {
        Finish{
            cache:self.cache,
            key: self.key,
            file: self.file
        }
    }

    pub fn roll_back(self) -> CleanUp {
        CleanUp{
            cache:self.cache,
            key: self.key,
        }
    }
}

pub struct CleanUp {
    pub(crate) cache: Arc<RwLock<CacheInner>>,
    pub(crate) key: Option<String>,
}

impl Future for CleanUp {
    type Item = ();
    type Error = Error;

    fn poll (&mut self) -> Poll<Self::Item, Self::Error> {
        match self.key.take() {
            None => panic!("Calling resolved future"),
            Some(key) => match blocking(|| {
                super::cleanup(&self.cache, &key)       
            }) {
                Err(e) => Err(e.into()),
                Ok(Async::Ready(())) => Ok(Async::Ready(())),
                Ok(Async::NotReady) => {
                    self.key = Some(key);
                    Ok(Async::NotReady)
                    },
            },
        }
    }
}

pub struct SaveIndex {
    pub(crate) cache: Arc<RwLock<CacheInner>>,
}

impl Future for SaveIndex {
    type Item = ();
    type Error = Error;

    fn poll (&mut self) -> Poll<Self::Item, Self::Error> {
            match blocking(|| {
                let cache = self.cache.write().unwrap();
                cache.save_index()

            }) {
                Err(e) => Err(e.into()),
                Ok(Async::Ready(Err(e))) => Err(e),
                Ok(Async::Ready(Ok(()))) => Ok(Async::Ready(())),
                Ok(Async::NotReady) => Ok(Async::NotReady)
                    
            }
    }
}




pub struct Finish {
    pub(crate) cache: Arc<RwLock<CacheInner>>,
    pub(crate) key: Option<String>,
    pub(crate) file: fs::File,
}

impl Future for Finish {
    type Item = ();
    type Error = Error;

    fn poll (&mut self) -> Poll<Self::Item, Self::Error> {
        match self.key.take() {
            None => panic!("Calling resolved future"),
            Some(key) => match blocking(|| {
                let mut c = self.cache.write().expect("Cannot lock cache");
                c.finish(key.clone(), &mut self.file)
                    
            }) {
                Err(e) => Err(e.into()),
                Ok(Async::Ready(Err(e))) => Err(e.into()),
                Ok(Async::Ready(Ok(r))) => Ok(Async::Ready(r)),
                Ok(Async::NotReady) => {
                    self.key = Some(key);
                    Ok(Async::NotReady)
                    },
            },
        }
    }
}

#[cfg(test)]
mod tests {
use tempfile::tempdir;
use crate::Cache;
use super::*;
#[test]
    fn test_async() {
        use tokio::prelude::*;
        env_logger::try_init().ok();
        const MY_KEY: &str = "muj_test_1";
        let temp_dir = tempdir().unwrap();

        let msg = String::from("Hello there");
        let msg2 = msg.clone();

        let c = Cache::new(temp_dir.path(), 10000, 10).unwrap();
        let c2 = c.clone();

        let fut = c.add_async(String::from(MY_KEY))
            .and_then(move |(w,fin)| { 
                tokio::io::write_all(w,msg.clone())
                .map_err(|e| e.into())
                .and_then(|_| fin.commit())
            })
            .and_then(move |_| {
                c.get_async(MY_KEY)
                .and_then(|maybe_file| {
                    match maybe_file {
                        None => panic!("cache file not found"),
                        Some(f) => {
                            tokio::io::read_to_end(f, Vec::new())
                            .map_err(|e| e.into())
                            .and_then(move |(_, res)| {
                                let s = std::str::from_utf8(&res).unwrap();
                                assert_eq!(msg2, s);
                                info!("ALL DONE");
                                Ok(())
                            })
                        }
                    }
                })
            })
            ;

            run_future(fut);

            c2.get(MY_KEY).unwrap().unwrap();
    }

    use tokio_threadpool::Builder;
    use futures::sync::oneshot;
    fn run_future<F>(f: F)
    where
        F: Future<Item = (), Error = Error> + Send + 'static,
    {
        let pool = Builder::new().pool_size(1).build();
        let (tx, rx) = oneshot::channel::<()>();
        pool.spawn(f.then(|res| tx.send(res.unwrap())));
        rx.wait().unwrap()
    }

}
