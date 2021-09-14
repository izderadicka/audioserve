use crate::{
    audio_folder::FolderLister,
    audio_meta::AudioFolder,
    error::{Error, Result},
    FoldersOrdering,
};
use notify::{watcher, Watcher};
use sled::Db;
use std::{
    convert::TryInto,
    path::{Path, PathBuf},
    sync::{mpsc::channel, Arc, Condvar, Mutex},
    thread,
    time::{Duration, SystemTime},
};
use walkdir::{DirEntry, WalkDir};

#[derive(Clone)]
struct CacheInner {
    db: Arc<Db>,
    lister: FolderLister,
    base_dir: PathBuf,
}

impl CacheInner {
    fn get<P: AsRef<Path>>(&self, dir: P) -> Option<AudioFolder> {
        dir.as_ref()
            .to_str()
            .and_then(|p| {
                self.db
                    .get(p)
                    .map_err(|e| error!("Cannot get record for db: {}", e))
                    .ok()
                    .flatten()
            })
            .and_then(|data| {
                bincode::deserialize(&data)
                    .map_err(|e| error!("Error deserializing data from db {}", e))
                    .ok()
            })
    }

    fn get_if_actual<P: AsRef<Path>>(&self, dir: P, ts: Option<SystemTime>) -> Option<AudioFolder> {
        let af = self.get(dir);
        af.as_ref()
            .and_then(|af| af.modified)
            .and_then(|cached_time| ts.map(|actual_time| cached_time >= actual_time))
            .and_then(|actual| if actual { af } else { None })
    }

    fn update<P: AsRef<Path>>(&self, dir: P, af: AudioFolder) -> Result<()> {
        let dir = dir
            .as_ref()
            .to_str()
            .ok_or_else(|| Error::InvalidCollectionPath)?;
        bincode::serialize(&af)
            .map_err(Error::from)
            .and_then(|data| self.db.insert(dir, data).map_err(Error::from))
            .map(|_| debug!("Cache updated for {:?}", dir))
    }

    fn force_update<P: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        base_dir: P,
        dir_path: P2,
    ) -> Result<()> {
        let af =
            self.lister
                .list_dir(base_dir, dir_path.as_ref(), FoldersOrdering::Alphabetical)?;
        self.update(dir_path, af)
    }

    fn full_path<P: AsRef<Path>>(&self, rel_path: P) -> PathBuf {
        self.base_dir.join(rel_path.as_ref())
    }
}

pub struct CollectionCache {
    thread: Option<thread::JoinHandle<()>>,
    cond: Arc<(Condvar, Mutex<bool>)>,
    inner: CacheInner,
}

impl CollectionCache {
    pub fn new<P1: Into<PathBuf>, P2: AsRef<Path>>(
        path: P1,
        db_dir: P2,
        lister: FolderLister,
    ) -> Result<CollectionCache> {
        let root_path = path.into();
        let db_path = CollectionCache::db_path(&root_path, db_dir)?;
        let db = sled::open(db_path)?;
        Ok(CollectionCache {
            inner: CacheInner {
                db: Arc::new(db),
                lister,
                base_dir: root_path,
            },
            thread: None,
            cond: Arc::new((Condvar::new(), Mutex::new(false))),
        })
    }

    pub fn list_dir<P: AsRef<Path>>(
        &self,
        dir_path: P,
        ordering: FoldersOrdering,
    ) -> Result<AudioFolder> {
        let full_path = self.inner.full_path(&dir_path);
        let ts = full_path.metadata().ok().and_then(|m| m.modified().ok());
        self.inner
            .get_if_actual(&dir_path, ts)
            .map(|mut af| {
                if matches!(ordering, FoldersOrdering::RecentFirst) {
                    af.subfolders
                        .sort_unstable_by(|a, b| a.compare_as(ordering, b));
                }
                af
            })
            .ok_or_else(|| {
                debug!(
                    "Fetching folder {:?} from file file system",
                    dir_path.as_ref()
                );
                self.inner
                    .lister
                    .list_dir(&self.inner.base_dir, &dir_path, ordering)
                    .map_err(Error::from)
            })
            .or_else(|r| {
                if let Ok(af_ref) = r.as_ref() {
                    // We should update cache as we got new info
                    debug!("Updating cache for dir {:?}", full_path);
                    let mut af = af_ref.clone();
                    if matches!(ordering, FoldersOrdering::RecentFirst) {
                        af.subfolders.sort_unstable_by(|a, b| {
                            a.compare_as(FoldersOrdering::Alphabetical, b)
                        });
                    }
                    self.inner
                        .update(dir_path, af)
                        .map_err(|e| error!("Cannot update collection: {}", e))
                        .ok();
                }
                r
            })
    }

    fn db_path<P1: AsRef<Path>, P2: AsRef<Path>>(path: P1, db_dir: P2) -> Result<PathBuf> {
        let p: &Path = path.as_ref();
        let path_hash = ring::digest::digest(&ring::digest::SHA256, p.to_string_lossy().as_bytes());
        let name_prefix = format!(
            "{:x}",
            u64::from_be_bytes(path_hash.as_ref()[..8].try_into().expect("Invalid size"))
        );
        let name = p
            .file_name()
            .map(|name| name.to_string_lossy() + "_" + name_prefix.as_ref())
            .ok_or_else(|| Error::InvalidCollectionPath)?;
        Ok(db_dir.as_ref().join(name.as_ref()))
    }

    pub fn run_update_loop(&mut self, root_path: PathBuf) {
        let inner = self.inner.clone();
        let cond = self.cond.clone();
        let thread = thread::spawn(move || {
            loop {
                let walker = WalkDir::new(&root_path).follow_links(false).into_iter();
                let (cond_var, cond_mtx) = &*cond;
                {
                    let mut started = cond_mtx.lock().unwrap();
                    *started = false;
                }
                let (tx, rx) = channel();
                let mut watcher = watcher(tx, Duration::from_secs(10))
                    .map_err(|e| error!("Failed to create fs watcher: {}", e));
                if let Ok(ref mut watcher) = watcher {
                    watcher
                        .watch(&root_path, notify::RecursiveMode::Recursive)
                        .map_err(|e| error!("failed to start watching: {}", e))
                        .ok();
                }

                // clean up non-exitent directories
                for key in inner.db.iter().filter_map(|e| e.ok()).map(|(k, _)| k) {
                    if let Ok(rel_path) = std::str::from_utf8(&key) {
                        let full_path = root_path.join(rel_path);
                        if !full_path.exists() {
                            debug!("Removing {:?} from collection cache db", full_path);
                            inner
                                .db
                                .remove(rel_path)
                                .map_err(|e| error!("cannot remove revord from db: {}", e))
                                .ok();
                        }
                    }
                }

                // inittial scan of directory
                for entry in walker.filter_entry(|e| is_visible_dir(e)) {
                    match entry {
                        Ok(entry) => {
                            let rel_path = entry
                                .path()
                                .strip_prefix(&root_path)
                                .expect("always have root path");
                            let mod_ts = entry.metadata().ok().and_then(|m| m.modified().ok());
                            if inner.get_if_actual(rel_path, mod_ts).is_none() {
                                match inner.lister.list_dir(
                                    &root_path,
                                    rel_path,
                                    FoldersOrdering::Alphabetical,
                                ) {
                                    Ok(af) => {
                                        inner
                                            .update(rel_path, af)
                                            .map_err(|e| error!("Cannot insert to db {}", e))
                                            .ok();
                                    }
                                    Err(e) => error!(
                                        "Cannot listing audio folder {:?}, error {}",
                                        entry.path(),
                                        e
                                    ),
                                }
                            } else {
                                debug!("For path {:?} using cached data", entry.path())
                            }
                        }
                        Err(e) => error!("Cannot read directory entry: {}", e),
                    }
                }

                // Notify about finish of initial scan
                {
                    let mut started = cond_mtx.lock().unwrap();
                    *started = true;
                    cond_var.notify_all();
                }

                // now update changed directories
                loop {
                    match rx.recv() {
                        Ok(event) => {
                            debug!("Change in collection {:?} => {:?}", root_path, event);
                            let paths_to_update = match event {
                                notify::DebouncedEvent::NoticeWrite(_) => continue,
                                notify::DebouncedEvent::NoticeRemove(_) => continue,
                                notify::DebouncedEvent::Create(p) => (p, None),
                                notify::DebouncedEvent::Write(p) => (p, None),
                                notify::DebouncedEvent::Chmod(_) => continue,
                                notify::DebouncedEvent::Remove(p) => (p, None),
                                notify::DebouncedEvent::Rename(p1, p2) => (p1, Some(p2)),
                                notify::DebouncedEvent::Rescan => {
                                    warn!("Rescaning of collection required");
                                    break;
                                }
                                notify::DebouncedEvent::Error(e, p) => {
                                    error!("Watch event error {} on {:?}", e, p);
                                    continue;
                                }
                            };
                        }
                        Err(e) => {
                            error!("Error in collection watcher channel: {}", e);
                            thread::sleep(Duration::from_secs(10));
                        }
                    }
                }
            }
        });
        self.thread = Some(thread);
    }

    pub fn wait_until_inital_scan_is_done(&self) {
        let (cond_var, cond_mtx) = &*self.cond;
        let mut started = cond_mtx.lock().unwrap();
        while !*started {
            started = cond_var.wait(started).unwrap();
        }
    }

    pub fn get<P: AsRef<Path>>(&self, dir: P) -> Option<AudioFolder> {
        self.inner.get(dir)
    }

    pub fn force_update<P: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        base_dir: P,
        dir_path: P2,
    ) -> Result<()> {
        self.inner.force_update(base_dir, dir_path)
    }

    pub fn flush(&self) -> Result<()> {
        self.inner.db.flush().map(|_| ()).map_err(Error::from)
    }
}

fn is_visible_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && !entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {

    use std::fs;

    use fs_extra::dir::{copy, CopyOptions};
    use tempdir::TempDir;

    use super::*;

    #[test]
    fn test_cache_creation() {
        env_logger::try_init().ok();
        let tmp_dir = TempDir::new("AS_CACHE_TEST").expect("Cannot create temp dir");
        let test_data_dir = Path::new("../../test_data");
        let db_path = tmp_dir.path().join("updater_db");
        let mut col = CollectionCache::new(test_data_dir, db_path, FolderLister::new())
            .expect("Cannot create CollectionCache");
        col.run_update_loop(test_data_dir.into());
        col.wait_until_inital_scan_is_done();

        let entry1 = col.get("").unwrap();
        let entry2 = col.get("usak/kulisak").unwrap();
        assert_eq!(2, entry1.files.len());
        assert_eq!(2, entry1.subfolders.len());
        assert_eq!(0, entry2.files.len())
    }

    #[test]
    fn test_cache_manipulation() -> anyhow::Result<()> {
        env_logger::try_init().ok();
        let tmp_dir = TempDir::new("AS_CACHE_TEST")?;
        let test_data_dir_orig = Path::new("../../test_data");
        let test_data_dir = tmp_dir.path().join("test_data");
        copy(&test_data_dir_orig, tmp_dir.path(), &CopyOptions::default())?;
        let info_file = test_data_dir.join("usak/kulisak/desc.txt");
        assert!(info_file.exists());
        let db_path = tmp_dir.path().join("updater_db");
        let col = CollectionCache::new(&test_data_dir, db_path, FolderLister::new())
            .expect("Cannot create CollectionCache");

        col.force_update(&test_data_dir, "usak/kulisak")?;
        let af = col.get("usak/kulisak").expect("cache record exits");
        let ts1 = af.modified.unwrap();
        assert_eq!(
            Path::new("usak/kulisak/desc.txt"),
            af.description.unwrap().path
        );
        let new_info_name = test_data_dir.join("usak/kulisak/info.txt");
        fs::rename(info_file, new_info_name)?;
        let af2 = col.list_dir("usak/kulisak", FoldersOrdering::RecentFirst)?;
        assert_eq!(
            Path::new("usak/kulisak/info.txt"),
            af2.description.unwrap().path
        );
        assert!(af2.modified.unwrap() >= ts1);

        Ok(())
    }

    #[test]
    fn test_db_path() {
        let path = Path::new("nejaka/cesta/na/kolekci");
        let collection_path = CollectionCache::db_path(path, "databaze").unwrap();
        let name = collection_path.file_name().unwrap().to_string_lossy();
        let name: Vec<_> = name.split('_').collect();
        assert_eq!("kolekci", name[0]);
        assert_eq!(16, name[1].len());
    }
}
