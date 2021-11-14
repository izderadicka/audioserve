use self::{
    inner::CacheInner,
    update::{OngoingUpdater, UpdateAction},
    util::kv_to_audiofolder,
};
use crate::{
    audio_folder::FolderLister,
    audio_meta::{AudioFolder, FolderByModification, TimeStamp},
    cache::update::{filter_event, FilteredEvent, RecursiveUpdater},
    common::{CollectionTrait, PositionsData, PositionsTrait},
    error::{Error, Result},
    position::{Position, PositionShort, PositionsCollector},
    util::get_modified,
    AudioFolderShort, FoldersOrdering,
};
use crossbeam_channel::{unbounded as channel, Receiver, Sender};
use notify::{watcher, DebouncedEvent, Watcher};
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
    watcher_sender: Arc<Mutex<Option<std::sync::mpsc::Sender<DebouncedEvent>>>>,
    thread_updater: Option<thread::JoinHandle<()>>,
    cond: Arc<(Condvar, Mutex<bool>)>,
    pub(crate) inner: Arc<CacheInner>,
    update_sender: Sender<Option<UpdateAction>>,
    update_receiver: Option<Receiver<Option<UpdateAction>>>,
    force_update: bool,
}

impl CollectionCache {
    pub fn new<P1: Into<PathBuf>, P2: AsRef<Path>>(
        path: P1,
        db_dir: P2,
        lister: FolderLister,
        force_update: bool,
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
            watcher_sender: Arc::new(Mutex::new(None)),
            thread_updater: None,

            cond: Arc::new((
                Condvar::new(),
                #[allow(clippy::mutex_atomic)]
                // Not sure why clippy warns, as this is taken from example cor condition in std doc
                Mutex::new(false),
            )),
            update_sender,
            update_receiver: Some(update_receiver),
            force_update,
        })
    }

    fn db_path<P1: AsRef<Path>, P2: AsRef<Path>>(path: P1, db_dir: P2) -> Result<PathBuf> {
        let p: PathBuf = path.as_ref().canonicalize()?;
        let key = p.to_string_lossy();
        let path_hash = ring::digest::digest(&ring::digest::SHA256, key.as_bytes());
        let name_prefix = format!(
            "{:016x}",
            u64::from_be_bytes(path_hash.as_ref()[..8].try_into().expect("Invalid size"))
        );
        let name = p
            .file_name()
            .map(|name| name.to_string_lossy() + "_" + name_prefix.as_ref())
            .ok_or(Error::InvalidCollectionPath)?;
        Ok(db_dir.as_ref().join(name.as_ref()))
    }

    pub(crate) fn restore_positions<P1: Into<PathBuf>, P2: AsRef<Path>>(
        path: P1,
        db_dir: P2,
        lister: FolderLister,
        force_update: bool,
        backup_data: PositionsData,
    ) -> Result<thread::JoinHandle<()>> {
        let col = CollectionCache::new(path, db_dir, lister, force_update)?;
        let inner = col.inner.clone();
        let thread = thread::spawn(move || {
            // clean up non-exitent directories
            inner.clean_up_folders();

            // inittial scan of directory
            let updater = RecursiveUpdater::new(&inner, None, force_update);
            updater.process();

            // clean up positions for non existent folders
            inner.clean_up_positions();

            inner
                .read_json_positions(backup_data)
                .map_err(|e| error!("Restore of collection {:?} failed: {}", inner.base_dir(), e))
                .ok();
        });
        Ok(thread)
    }

    pub(crate) fn run_update_loop(&mut self) {
        let update_receiver = self.update_receiver.take().expect("run multiple times");
        let inner = self.inner.clone();
        let ongoing_updater = OngoingUpdater::new(update_receiver, inner.clone());
        self.thread_updater = Some(thread::spawn(move || ongoing_updater.run()));
        let cond = self.cond.clone();
        let mut force_update = self.force_update;
        let watcher_sender = self.watcher_sender.clone();

        let thread = thread::spawn(move || {
            let root_path = inner.base_dir();
            loop {
                let (cond_var, cond_mtx) = &*cond;
                {
                    let mut started = cond_mtx.lock().unwrap();
                    *started = false;
                }
                // Not ready to receive reload signals until scan is done
                {
                    let mut ws = watcher_sender.lock().unwrap();
                    *ws = None;
                }
                let (tx, rx) = std::sync::mpsc::channel();

                let mut watcher = watcher(tx.clone(), Duration::from_secs(1))
                    .map_err(|e| error!("Failed to create fs watcher: {}", e));
                if let Ok(ref mut watcher) = watcher {
                    watcher
                        .watch(&root_path, notify::RecursiveMode::Recursive)
                        .map_err(|e| error!("failed to start watching: {}", e))
                        .ok();
                }

                // clean up non-exitent directories
                inner.clean_up_folders();

                // inittial scan of directory
                let updater = RecursiveUpdater::new(&inner, None, force_update);
                updater.process();
                force_update = false;

                // clean up positions for non existent folders
                inner.clean_up_positions();

                // Notify about finish of initial scan
                {
                    let mut started = cond_mtx.lock().unwrap();
                    *started = true;
                    cond_var.notify_all();
                }
                // And can wait for rescan signal
                {
                    let mut ws = watcher_sender.lock().unwrap();
                    *ws = Some(tx);
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
                                    info!("Rescaning of collection required");
                                    force_update = true;
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

    #[allow(dead_code)]
    pub fn force_update<P: AsRef<Path>>(&self, dir_path: P) -> Result<()> {
        self.inner.force_update(dir_path, false).map(|_| ())
    }
}

impl CollectionTrait for CollectionCache {
    fn list_dir<P: AsRef<Path>>(
        &self,
        dir_path: P,
        ordering: FoldersOrdering,
        group: Option<String>,
    ) -> Result<AudioFolder> {
        let dir_path = dir_path.as_ref();
        let full_path = self.inner.full_path(dir_path);
        let ts = get_modified(&full_path);
        self.inner
            .get_if_actual(dir_path, ts)
            .map(|mut af| {
                if matches!(ordering, FoldersOrdering::RecentFirst) {
                    af.subfolders
                        .sort_unstable_by(|a, b| a.compare_as(ordering, b));
                }
                af
            })
            .ok_or_else(|| {
                debug!("Fetching folder {:?} from file system", dir_path);
                self.inner.list_dir(dir_path, ordering)
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
            .map(|mut af| {
                if let Some(group) = group {
                    let folder = dir_path.to_str();
                    if let Some(folder) = folder {
                        let pos = self.get_position(&group, Some(folder)).and_then(|p| {
                            dir_path.join(&p.file).to_str().map(|path| PositionShort {
                                path: path.to_string(),
                                timestamp: p.timestamp,
                                position: p.position,
                            })
                        });
                        af.position = pos;
                        self.inner.update_subfolders(group, &mut af.subfolders)
                    } else {
                        warn!(
                            "Folder path {:?} is not UTF8, cannot get position",
                            dir_path
                        )
                    }
                }
                af
            })
    }

    fn flush(&self) -> Result<()> {
        self.inner.flush()
    }

    fn search<S: AsRef<str>>(&self, q: S) -> Vec<AudioFolderShort> {
        let tokens: Vec<String> = q
            .as_ref()
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();
        let iter = self.inner.iter_folders();
        let search = Search {
            tokens,
            iter,
            prev_match: None,
        };
        search.collect()
    }

    fn recent(&self, limit: usize) -> Vec<AudioFolderShort> {
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

    fn signal_rescan(&self) {
        if let Ok(tx) = self.watcher_sender.lock() {
            tx.as_ref()
                .and_then(|tx| tx.send(DebouncedEvent::Rescan).ok());
        }
    }

    fn base_dir(&self) -> &Path {
        self.inner.base_dir()
    }
}

impl Drop for CollectionCache {
    fn drop(&mut self) {
        self.update_sender.send(None).ok();
        if let Some(t) = self.thread_updater.take() {
            t.join().ok();
            debug!("Update thread joined");
        } else {
            warn!("Join handle is missing");
        }
        self.inner
            .flush()
            .map_err(|e| error!("Final flush failed: {}", e))
            .ok();
    }
}

// positions
impl PositionsTrait for CollectionCache {
    fn insert_position<S, P>(
        &self,
        group: S,
        path: P,
        position: f32,
        finished: bool,
        ts: Option<TimeStamp>,
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.inner
            .insert_position(group, path, position, finished, ts, false)
    }

    fn get_position<S, P>(&self, group: S, folder: Option<P>) -> Option<Position>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.inner.get_position(group, folder)
    }

    fn get_all_positions_for_group<S>(
        &self,
        group: S,
        collection_no: usize,
        res: &mut PositionsCollector,
    ) where
        S: AsRef<str>,
    {
        self.inner
            .get_all_positions_for_group(group, collection_no, res)
    }

    fn write_json_positions<F: std::io::Write>(&self, file: &mut F) -> Result<()> {
        self.inner.write_json_positions(file)
    }

    fn read_json_positions(&self, data: PositionsData) -> Result<()> {
        self.inner.read_json_positions(data)
    }

    fn get_positions_recursive<S, P>(
        &self,
        group: S,
        folder: P,
        collection_no: usize,
        res: &mut PositionsCollector,
    ) where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.inner
            .get_positions_recursive(group, folder, collection_no, res)
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
        for item in &mut self.iter {
            match item {
                Ok((key, val)) => {
                    let path = std::str::from_utf8(key.as_ref()).unwrap(); // we can safely unwrap as we inserted string
                    if self
                        .prev_match
                        .as_ref()
                        .and_then(|m| path.strip_prefix(m))
                        .map(|s| s.contains(std::path::MAIN_SEPARATOR)) // only match was parent path
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

    use std::{
        fs::{self, File},
        io::{Read, Write},
        time::SystemTime,
    };

    use fs_extra::dir::{copy, CopyOptions};
    use serde_json::Value;
    use tempdir::TempDir;

    use crate::position::PositionItem;

    use super::*;

    fn create_tmp_collection() -> (CollectionCache, TempDir) {
        let tmp_dir = TempDir::new("AS_CACHE_TEST").expect("Cannot create temp dir");
        let test_data_dir = Path::new("../../test_data");
        let db_path = tmp_dir.path().join("updater_db");
        let mut col = CollectionCache::new(test_data_dir, db_path, FolderLister::new(), false)
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
        let col = CollectionCache::new(&test_data_dir, db_path, FolderLister::new(), false)
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
        let af2 = col.list_dir("usak/kulisak", FoldersOrdering::RecentFirst, None)?;
        assert_eq!(
            Path::new("usak/kulisak/info.txt"),
            af2.description.unwrap().path
        );
        assert!(af2.modified.unwrap() >= ts1);

        Ok(())
    }

    #[test]
    fn test_positions_json() -> anyhow::Result<()> {
        env_logger::try_init().ok();
        let (col, tmp_dir) = create_tmp_collection();
        col.insert_position("ivan", "02-file.opus", 1.0, false, None)?;
        col.insert_position(
            "ivan",
            "01-file.mp3/002 - Chapter 3$$2000-3000$$.mp3",
            0.04,
            false,
            None,
        )?;
        let fname = tmp_dir.path().join("pos.json");
        let mut backup_file = File::create(fname.clone())?;
        col.write_json_positions(&mut backup_file)?;
        backup_file.flush()?;
        drop(backup_file);
        let mut f = File::open(fname)?;
        let mut data = String::new();
        let read = f.read_to_string(&mut data)?;
        assert!(read > 40);
        println!("DATA:\n {}", data);
        let json = serde_json::from_str::<serde_json::Map<_, _>>(&data)?;
        assert_eq!(2, json.len());
        let v = json
            .get("")
            .and_then(|v| {
                if let Value::Object(map) = v {
                    map.get("ivan")
                } else {
                    None
                }
            })
            .unwrap();
        let pos: PositionItem = serde_json::from_value(v.clone())?;
        assert_eq!(1.0, pos.position);

        // recovery to same collection should work
        col.read_json_positions(PositionsData::V1(json.clone()))?;

        // and also it should work for new collection
        let (col2, _tmp2) = create_tmp_collection();
        assert!(col2.get_position::<_, String>("ivan", None).is_none());
        col2.read_json_positions(PositionsData::V1(json))?;
        let mut res = PositionsCollector::new(100);
        col2.get_all_positions_for_group("ivan", 0, &mut res);
        assert_eq!(2, res.into_vec().len());
        Ok(())
    }

    #[test]
    fn test_db_path() {
        let path = Path::new("../../test_data/usak");
        let collection_path = CollectionCache::db_path(path, "databaze").unwrap();
        let name = collection_path.file_name().unwrap().to_string_lossy();
        let name: Vec<_> = name.split('_').collect();
        assert_eq!("usak", name[0]);
        assert_eq!(16, name[1].len());
    }

    #[test]
    fn test_search() {
        env_logger::try_init().ok();
        let (col, _tmp_dir) = create_tmp_collection();
        let res: Vec<_> = col.search("usak kulisak");
        assert_eq!(1, res.len());
        let af = &res[0];
        assert_eq!("kulisak", af.name.as_str());
        let corr_path = Path::new("usak").join("kulisak");
        assert_eq!(corr_path, af.path);
        assert!(af.modified.is_some());
        assert!(!af.is_file);

        let res: Vec<_> = col.search("neneneexistuje");
        assert_eq!(0, res.len());
    }

    #[test]
    fn test_position() -> anyhow::Result<()> {
        env_logger::try_init().ok();
        let (col, _tmp_dir) = create_tmp_collection();
        col.insert_position("ivan", "02-file.opus", 1.0, false, None)?;
        let r1 = col
            .get_position("ivan", Some(""))
            .expect("position record exists");
        assert_eq!(r1.file, "02-file.opus");
        assert_eq!(r1.position, 1.0);
        col.insert_position(
            "ivan",
            "01-file.mp3/002 - Chapter 3$$2000-3000$$.mp3",
            0.04,
            false,
            None,
        )?;
        // test insert position with old timestamp, should not be inserted
        let ts: TimeStamp = (SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 10 * 1000)
            .into();
        let res = col.insert_position(
            "ivan",
            "01-file.mp3/002 - Chapter 3$$2000-3000$$.mp3",
            0.08,
            false,
            Some(ts),
        );
        assert!(res.is_err());
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
            false,
            Some(ts),
        )?;

        let r3 = col
            .get_position::<_, &str>("ivan", None)
            .expect("last position exists");
        assert_eq!(r3.file, "002 - Chapter 3$$2000-3000$$.mp3");
        assert_eq!(r3.position, 0.08);

        // test listing all positions
        let mut res = PositionsCollector::new(10);
        col.inner.get_all_positions_for_group("ivan", 0, &mut res);
        assert_eq!(2, res.into_vec().len());

        let mut res = PositionsCollector::new(10);
        col.inner.get_positions_recursive("ivan", "", 0, &mut res);
        assert_eq!(2, res.into_vec().len());

        let mut res = PositionsCollector::new(10);
        col.inner
            .get_positions_recursive("ivan", "01-file.mp3", 0, &mut res);
        assert_eq!(1, res.into_vec().len());
        Ok(())
    }
}
