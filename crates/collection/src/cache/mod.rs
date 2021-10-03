use self::{
    inner::CacheInner,
    update::{OngoingUpdater, UpdateAction},
    util::kv_to_audiofolder,
};
use crate::{
    audio_folder::FolderLister,
    audio_meta::{AudioFolder, FolderByModification, TimeStamp},
    cache::update::{filter_event, FilteredEvent, RecursiveUpdater},
    error::{Error, Result},
    position::Position,
    util::get_modified,
    AudioFolderShort, FoldersOrdering,
};
use crossbeam_channel::{unbounded as channel, Receiver, Sender};
use notify::{watcher, Watcher};
use std::{
    collections::BinaryHeap,
    convert::TryInto,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

mod inner;
mod update;
mod util;

pub struct CollectionCache {
    thread_loop: Option<thread::JoinHandle<()>>,
    thread_updater: Option<thread::JoinHandle<()>>,
    cond: Arc<(Condvar, Mutex<bool>)>,
    pub(crate) inner: Arc<CacheInner>,
    update_sender: Sender<Option<UpdateAction>>,
    update_receiver: Option<Receiver<Option<UpdateAction>>>,
}

impl CollectionCache {
    pub fn new<P1: Into<PathBuf>, P2: AsRef<Path>>(
        path: P1,
        db_dir: P2,
        lister: FolderLister,
    ) -> Result<CollectionCache> {
        let root_path = path.into();
        let db_path = CollectionCache::db_path(&root_path, &db_dir)?;
        let db = sled::Config::default()
            .path(&db_path)
            .use_compression(true)
            .flush_every_ms(Some(10_000))
            .cache_capacity(100 * 1024 * 1024)
            .open()?;
        let (update_sender, update_receiver) = channel::<Option<UpdateAction>>();
        Ok(CollectionCache {
            inner: Arc::new(CacheInner::new(
                db,
                lister,
                root_path,
                update_sender.clone(),
            )?),
            thread_loop: None,
            thread_updater: None,
            cond: Arc::new((Condvar::new(), Mutex::new(false))),
            update_sender,
            update_receiver: Some(update_receiver),
        })
    }

    pub fn list_dir<P: AsRef<Path>>(
        &self,
        dir_path: P,
        ordering: FoldersOrdering,
    ) -> Result<AudioFolder> {
        let full_path = self.inner.full_path(&dir_path);
        let ts = get_modified(&full_path);
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
                debug!("Fetching folder {:?} from file system", dir_path.as_ref());
                self.inner.list_dir(&dir_path, ordering)
            })
            .or_else(|r| {
                match r.as_ref() {
                    Ok(af_ref) => {
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
                    Err(e) => {
                        error!("Got error when fetching folder from file system: {}", e);
                        // let parent = parent_path(dir_path);
                        // self.force_update(parent).map_err(|e| error!("Update of parent dir failed: {}", e)).ok();
                    }
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

    pub fn run_update_loop(&mut self) {
        let update_receiver = self.update_receiver.take().expect("run multiple times");
        let inner = self.inner.clone();
        let ongoing_updater = OngoingUpdater::new(update_receiver, inner.clone());
        self.thread_updater = Some(thread::spawn(move || ongoing_updater.run()));
        let cond = self.cond.clone();

        let thread = thread::spawn(move || {
            let root_path = inner.base_dir();
            loop {
                let (cond_var, cond_mtx) = &*cond;
                {
                    let mut started = cond_mtx.lock().unwrap();
                    *started = false;
                }
                let (tx, rx) = std::sync::mpsc::channel();
                let mut watcher = watcher(tx, Duration::from_secs(1))
                    .map_err(|e| error!("Failed to create fs watcher: {}", e));
                if let Ok(ref mut watcher) = watcher {
                    watcher
                        .watch(&root_path, notify::RecursiveMode::Recursive)
                        .map_err(|e| error!("failed to start watching: {}", e))
                        .ok();
                }

                // clean up non-exitent directories
                for key in inner.iter_folders().filter_map(|e| e.ok()).map(|(k, _)| k) {
                    if let Ok(rel_path) = std::str::from_utf8(&key) {
                        let full_path = root_path.join(rel_path);
                        if !full_path.exists() {
                            debug!("Removing {:?} from collection cache db", full_path);
                            inner
                                .remove(rel_path)
                                .map_err(|e| error!("cannot remove revord from db: {}", e))
                                .ok();
                        }
                    }
                }

                // inittial scan of directory
                let mut updater = RecursiveUpdater::new(&inner, None);
                updater.process();

                // Notify about finish of initial scan
                {
                    let mut started = cond_mtx.lock().unwrap();
                    *started = true;
                    cond_var.notify_all();
                }

                info!(
                    "Initial scan for collection {:?} finished",
                    inner.base_dir()
                );

                // now update changed directories
                loop {
                    match rx.recv() {
                        Ok(event) => {
                            trace!("Change in collection {:?} => {:?}", root_path, event);
                            let interesting_event = match filter_event(event) {
                                FilteredEvent::Ignore => continue,
                                FilteredEvent::Pass(evt) => evt,
                                FilteredEvent::Rescan => {
                                    warn!("Rescaning of collection required");
                                    break;
                                }
                                FilteredEvent::Error(e, p) => {
                                    error!("Watch event error {} on {:?}", e, p);
                                    continue;
                                }
                            };
                            inner.proceed_event(interesting_event)
                        }
                        Err(e) => {
                            error!("Error in collection watcher channel: {}", e);
                            thread::sleep(Duration::from_secs(10));
                            break;
                        }
                    }
                }
            }
        });
        self.thread_loop = Some(thread);
    }

    #[allow(dead_code)]
    pub fn wait_until_inital_scan_is_done(&self) {
        let (cond_var, cond_mtx) = &*self.cond;
        let mut started = cond_mtx.lock().unwrap();
        while !*started {
            started = cond_var.wait(started).unwrap();
        }
    }

    #[allow(dead_code)]
    pub fn get<P: AsRef<Path>>(&self, dir: P) -> Option<AudioFolder> {
        self.inner.get(dir)
    }

    pub fn force_update<P: AsRef<Path>>(&self, dir_path: P) -> Result<()> {
        self.inner.force_update(dir_path, false).map(|_| ())
    }

    pub fn flush(&self) -> Result<()> {
        self.inner.flush()
    }

    pub fn search<S: AsRef<str>>(&self, q: S) -> Search {
        let tokens: Vec<String> = q
            .as_ref()
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();
        let iter = self.inner.iter_folders();
        Search {
            tokens,
            iter,
            prev_match: None,
        }
    }

    pub fn recent(&self, limit: usize) -> Vec<AudioFolderShort> {
        let mut heap = BinaryHeap::with_capacity(limit + 1);

        for (key, val) in self.inner.iter_folders().skip(1).filter_map(|r| r.ok()) {
            let sf = kv_to_audiofolder(std::str::from_utf8(&key).unwrap(), val);
            heap.push(FolderByModification::from(sf));
            if heap.len() > limit {
                heap.pop();
            }
        }
        heap.into_sorted_vec()
            .into_iter()
            .map(|i| i.into())
            .collect()
    }
}

// positions
impl CollectionCache {
    pub fn insert_position<S, P>(
        &self,
        group: S,
        path: P,
        position: f32,
        ts: Option<TimeStamp>,
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.inner.insert_position(group, path, position, ts)
    }

    pub fn get_position<S, P>(&self, group: S, folder: Option<P>) -> Option<Position>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.inner.get_position(group, folder)
    }
}

// positions async
#[cfg(feature = "async")]
use tokio::task::spawn_blocking;

#[cfg(feature = "async")]
impl CollectionCache {
    pub async fn insert_position_async<S, P>(
        &self,
        group: S,
        path: P,
        position: f32,
        ts: Option<TimeStamp>,
    ) -> Result<()>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        let inner = self.inner.clone();
        spawn_blocking(move || inner.insert_position(group, path, position, ts))
            .await
            .map_err(Error::from)?
    }

    pub async fn get_position_async<S, P>(&self, group: S, path: Option<P>) -> Option<Position>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        let inner = self.inner.clone();
        spawn_blocking(move || inner.get_position(group, path))
            .await
            .map_err(|e| error!("Tokio join error: {}", e))
            .ok()
            .flatten()
    }
}

pub struct Search {
    tokens: Vec<String>,
    iter: sled::Iter,
    prev_match: Option<String>,
}

impl Iterator for Search {
    type Item = AudioFolderShort;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(item) = self.iter.next() {
            match item {
                Ok((key, val)) => {
                    let path = std::str::from_utf8(key.as_ref()).unwrap(); // we can safely unwrap as we inserted string
                    if self
                        .prev_match
                        .as_ref()
                        .map(|m| path.starts_with(m))
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    let path_lower_case = path.to_lowercase();
                    let is_match = self.tokens.iter().all(|t| path_lower_case.contains(t));
                    if is_match {
                        self.prev_match = Some(path.to_owned());
                        return Some(kv_to_audiofolder(path, val));
                    }
                }
                Err(e) => error!("Error iterating collection db: {}", e),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {

    use std::{fs, time::SystemTime};

    use fs_extra::dir::{copy, CopyOptions};
    use tempdir::TempDir;

    use super::*;

    fn create_tmp_collection() -> (CollectionCache, TempDir) {
        let tmp_dir = TempDir::new("AS_CACHE_TEST").expect("Cannot create temp dir");
        let test_data_dir = Path::new("../../test_data");
        let db_path = tmp_dir.path().join("updater_db");
        let mut col = CollectionCache::new(test_data_dir, db_path, FolderLister::new())
            .expect("Cannot create CollectionCache");
        col.run_update_loop();
        col.wait_until_inital_scan_is_done();
        (col, tmp_dir)
    }

    #[test]
    fn test_cache_creation() {
        env_logger::try_init().ok();
        let (col, _tmp_dir) = create_tmp_collection();

        let entry1 = col.get("").unwrap();
        let entry2 = col.get("usak/kulisak").unwrap();
        assert_eq!(2, entry1.files.len());
        assert_eq!(2, entry1.subfolders.len());
        assert_eq!(0, entry2.files.len());

        let entry3 = col.get("01-file.mp3").unwrap();
        assert_eq!(3, entry3.files.len());
        assert_eq!(0, entry3.subfolders.len());
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

        col.force_update("usak/kulisak")?;
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

    #[test]
    fn test_search() {
        env_logger::try_init().ok();
        let (col, _tmp_dir) = create_tmp_collection();
        let res: Vec<_> = col.search("usak kulisak").collect();
        assert_eq!(1, res.len());
        let af = &res[0];
        assert_eq!("kulisak", af.name.as_str());
        let corr_path = Path::new("usak").join("kulisak");
        assert_eq!(corr_path, af.path);
        assert!(af.modified.is_some());
        assert!(!af.is_file);

        let res: Vec<_> = col.search("neneneexistuje").collect();
        assert_eq!(0, res.len());
    }

    #[test]
    fn test_position() -> anyhow::Result<()> {
        env_logger::try_init().ok();
        let (col, _tmp_dir) = create_tmp_collection();
        col.insert_position("ivan", "02-file.opus", 1.0, None)?;
        let r1 = col
            .get_position("ivan", Some(""))
            .expect("position record exists");
        assert_eq!(r1.file, "02-file.opus");
        assert_eq!(r1.position, 1.0);
        col.insert_position(
            "ivan",
            "01-file.mp3/002 - Chapter 3$$2000-3000$$.mp3",
            0.04,
            None,
        )?;
        // test insert position with old timestamp, should not be inserted
        let ts: TimeStamp = (SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 10 * 1000)
            .into();
        col.insert_position(
            "ivan",
            "01-file.mp3/002 - Chapter 3$$2000-3000$$.mp3",
            0.08,
            Some(ts),
        )?;
        let r2 = col
            .get_position("ivan", Some("01-file.mp3"))
            .expect("position record exists");
        assert_eq!(r2.file, "002 - Chapter 3$$2000-3000$$.mp3");
        assert_eq!(r2.position, 0.04);

        // test insert position with current timestamp, should be inserted
        let ts: TimeStamp = (SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64)
            .into();
        col.insert_position(
            "ivan",
            "01-file.mp3/002 - Chapter 3$$2000-3000$$.mp3",
            0.08,
            Some(ts),
        )?;

        let r3 = col
            .get_position::<_, &str>("ivan", None)
            .expect("last position exists");
        assert_eq!(r3.file, "002 - Chapter 3$$2000-3000$$.mp3");
        assert_eq!(r3.position, 0.08);
        Ok(())
    }
}
