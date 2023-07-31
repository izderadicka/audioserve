extern crate bit_vec;
extern crate ego_tree;
extern crate notify;
#[macro_use]
extern crate log;
#[macro_use]
extern crate derive_builder;

pub use self::tree::{DirTree, SearchResult};
use self::utils::{Cond, CondAll};
use notify::EventKind;
use notify::{recommended_watcher, RecursiveMode, Watcher};
use std::borrow;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

mod tree;
mod utils;

#[derive(Clone, Copy, Builder)]
#[builder(default)]
pub struct Options {
    include_files: bool,
    watch_changes: bool,
    watch_recursively: bool,
    watch_delay: u64,
    follow_symlinks: bool,
    recent_list_size: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            include_files: true,
            watch_changes: false,
            watch_recursively: true,
            watch_delay: 10,
            follow_symlinks: false,
            recent_list_size: 0,
        }
    }
}

#[derive(Clone)]
pub struct DirCache {
    inner: Arc<DirCacheInner>,
}

impl DirCache {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        DirCache::new_with_options(root, Default::default())
    }

    pub fn new_with_options<P: AsRef<Path>>(root: P, options: Options) -> Self {
        let dc = DirCache {
            inner: Arc::new(DirCacheInner::new_with_options(&root, options)),
        };

        if options.watch_changes {
            let dc = dc.clone();
            let dc2 = dc.clone();
            let root: PathBuf = root.as_ref().into();
            let cond = Cond::new();
            let cond2 = cond.clone();

            let _watcher = thread::spawn(move || match dc.load() {
                Ok(_) => {
                    let (tx, rx) = channel();
                    let mut watcher = recommended_watcher(tx).unwrap();
                    let timeout = Duration::from_secs(options.watch_delay);
                    watcher
                        .watch(
                            &root,
                            if options.watch_recursively {
                                RecursiveMode::Recursive
                            } else {
                                RecursiveMode::NonRecursive
                            },
                        )
                        .unwrap();
                    let mut deadline: Option<Instant> = None;

                    macro_rules! update_event {
                        ($evt: expr) => {
                            match $evt {
                                Ok(event) => {
                                    debug!("directory change - event {:?}", event);
                                    match event.kind {
                                        EventKind::Create(_)
                                        | EventKind::Remove(_)
                                        | EventKind::Modify(_) => {
                                            deadline = Some(Instant::now() + timeout)
                                        }
                                        _ => (),
                                    }
                                }
                                Err(e) => error!("watch error: {:?}", e),
                            }
                        };
                    }

                    loop {
                        match deadline {
                            Some(d) => match rx.recv_timeout(d.duration_since(Instant::now())) {
                                Ok(evt) => update_event!(evt),
                                Err(RecvTimeoutError::Timeout) => {
                                    debug!("Directory change debounced");
                                    cond.notify();
                                    deadline = None;
                                }
                                Err(RecvTimeoutError::Disconnected) => {
                                    error!("watcher disconnected");
                                    break;
                                }
                            },
                            None => match rx.recv() {
                                Ok(evt) => update_event!(evt),
                                Err(_) => {
                                    error!("watcher disconnected");
                                    break;
                                }
                            },
                        }
                    }
                }
                Err(e) => error!("cannot start watching directory due to error: {}", e),
            });

            let _updater = thread::spawn(move || loop {
                cond2.wait();
                match dc2.load() {
                    Ok(_) => debug!("Directory cache updated"),
                    Err(e) => error!("Failed to update directory cache: error {}", e),
                }
            });
        }
        dc
    }

    pub fn is_ready(&self) -> bool {
        self.inner.cache.read().unwrap().is_some()
    }

    pub fn load(&self) -> Result<(), io::Error> {
        self.inner.load()
    }

    pub fn search<S: AsRef<str>>(&self, query: S) -> Result<Vec<PathBuf>, io::Error> {
        self.inner.search(query)
    }

    pub fn search_collected<S, F, T>(&self, query: S, collector: F) -> Result<T, io::Error>
    where
        S: AsRef<str>,
        F: FnOnce(SearchResult) -> T,
    {
        self.inner.search_collected(query, collector)
    }

    pub fn recent(&self) -> Result<Vec<PathBuf>, io::Error> {
        self.inner.recent()
    }

    pub fn wait_ready(&self) {
        self.inner.wait_ready()
    }
}
struct DirCacheInner {
    cache: RwLock<Option<DirTree>>,
    root: PathBuf,
    options: Options,
    ready_cond: CondAll,
}

impl DirCacheInner {
    fn new_with_options<P: AsRef<Path>>(root: P, options: Options) -> Self {
        DirCacheInner {
            root: root.as_ref().into(),
            cache: RwLock::new(None),
            options,
            ready_cond: CondAll::new(),
        }
    }

    fn wait_ready(&self) {
        self.ready_cond.wait()
    }

    fn load(&self) -> Result<(), io::Error> {
        let tree = DirTree::new_with_options(&self.root, self.options)?;
        {
            let mut cache = self.cache.write().unwrap();
            *cache = Some(tree)
        }
        self.ready_cond.notify_all();
        Ok(())
    }

    fn search<S: AsRef<str>>(&self, query: S) -> Result<Vec<PathBuf>, io::Error> {
        let cache = self.cache.read().unwrap();
        if cache.is_none() {
            return Err(io::Error::new(io::ErrorKind::Other, "cache not ready"));
        }
        Ok(cache
            .as_ref()
            .unwrap()
            .search(query)
            .map(|e| e.path())
            .collect())
    }

    fn search_collected<S, F, T>(&self, query: S, collector: F) -> Result<T, io::Error>
    where
        S: AsRef<str>,
        F: FnOnce(SearchResult) -> T,
    {
        let cache = self.cache.read().unwrap();
        if cache.is_none() {
            return Err(io::Error::new(io::ErrorKind::Other, "cache not ready"));
        }
        Ok(collector(cache.as_ref().unwrap().search(query)))
    }

    fn recent(&self) -> Result<Vec<PathBuf>, io::Error> {
        let cache = self.cache.read().unwrap();
        if cache.is_none() {
            return Err(io::Error::new(io::ErrorKind::Other, "cache not ready"));
        }
        let recent = cache.as_ref().unwrap().recent();
        match recent {
            Some(iter) => Ok(iter.map(borrow::ToOwned::to_owned).collect()),
            None => Err(io::Error::new(io::ErrorKind::Other, "recent not supported")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache() {
        let c = DirCache::new("test_data");
        assert!(!c.is_ready());
        c.load().unwrap();
        assert!(c.is_ready());
        let res = c.search("cargo").unwrap();
        assert_eq!(4, res.len())
    }

    #[test]
    fn test_search_collected() {
        let c = DirCache::new("test_data");
        c.load().unwrap();
        let res = c
            .search_collected("chesterton modry", |iter| {
                iter.map(|i| i.path())
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap();
        assert_eq!(1, res.len());
    }
    #[test]
    fn multithread() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::thread;
        let counter = Arc::new(AtomicUsize::new(0));
        const NUM_THREADS: usize = 100;
        let c = DirCache::new("test_data");
        let c2 = c.clone();
        assert!(!c.is_ready());
        let loader_thread = thread::spawn(move || {
            c.load().unwrap();
        });
        let mut threads = vec![];
        for _i in 0..NUM_THREADS {
            let c = c2.clone();
            let counter = counter.clone();
            let t = thread::spawn(move || {
                c.wait_ready();
                let res = c.search("cargo").unwrap();
                assert_eq!(4, res.len());
                counter.fetch_add(1, Ordering::Relaxed);
            });
            threads.push(t);
        }
        loader_thread.join().unwrap();
        for t in threads {
            t.join().unwrap()
        }

        assert_eq!(NUM_THREADS, counter.load(Ordering::Relaxed));
    }
}
