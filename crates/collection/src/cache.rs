use crate::{
    audio_folder::FolderLister,
    audio_meta::{AudioFolder, TimeStamp},
    error::{Error, Result},
    position::{Position, PositionItem, PositionRecord},
    AudioFolderShort, FoldersOrdering,
};
use crossbeam_channel::{unbounded as channel, Receiver, RecvTimeoutError, Sender};
use notify::{watcher, DebouncedEvent, Watcher};
use sled::{
    transaction::{self, TransactionError},
    Db, Transactional, Tree,
};
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, VecDeque},
    convert::TryInto,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, SystemTime},
};

const MAX_GROUPS: usize = 100;

fn deser_audiofoler<T: AsRef<[u8]>>(data: T) -> Option<AudioFolder> {
    bincode::deserialize(data.as_ref())
        .map_err(|e| error!("Error deserializing data from db {}", e))
        .ok()
}

fn kv_to_audiofolder<K: AsRef<str>, V: AsRef<[u8]>>(key: K, val: V) -> AudioFolderShort {
    let path = Path::new(key.as_ref());
    let folder = deser_audiofoler(val);
    AudioFolderShort {
        name: path.file_name().unwrap().to_string_lossy().into(),
        path: path.into(),
        is_file: folder.as_ref().map(|f| f.is_file).unwrap_or(false),
        modified: folder.as_ref().and_then(|f| f.modified),
    }
}

#[derive(Clone)]
pub(crate) struct CacheInner {
    db: Db,
    pos_latest: Tree,
    pos_folder: Tree,
    lister: FolderLister,
    base_dir: PathBuf,
    update_sender: Sender<Option<UpdateAction>>,
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
            .and_then(deser_audiofoler)
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

    fn force_update<P: AsRef<Path>>(&self, dir_path: P) -> Result<()> {
        let af = self.lister.list_dir(
            &self.base_dir,
            dir_path.as_ref(),
            FoldersOrdering::Alphabetical,
        )?;
        self.update(dir_path, af)
    }

    fn full_path<P: AsRef<Path>>(&self, rel_path: P) -> PathBuf {
        self.base_dir.join(rel_path.as_ref())
    }
}

// positions
impl CacheInner {
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
        (&self.pos_latest, &self.pos_folder)
            .transaction(|(pos_latest, pos_folder)| {
                let (path, file) = split_path(&path);

                let mut folder_rec = pos_folder
                    .get(&path)
                    .map_err(|e| error!("Db get error: {}", e))
                    .ok()
                    .flatten()
                    .and_then(|data| {
                        bincode::deserialize::<PositionRecord>(&data)
                            .map_err(|e| error!("Db item deserialization error: {}", e))
                            .ok()
                    })
                    .unwrap_or_else(HashMap::new);

                if let Some(ts) = ts {
                    if let Some(current_record) = folder_rec.get(group.as_ref()) {
                        if current_record.timestamp > ts {
                            return Ok(());
                        }
                    }
                }

                let this_pos = PositionItem {
                    file: file,
                    timestamp: TimeStamp::now(),
                    position,
                    folder_finished: false,
                };

                if !folder_rec.contains_key(group.as_ref()) && folder_rec.len() >= MAX_GROUPS {
                    return transaction::abort(Error::TooManyGroups);
                }

                folder_rec.insert(group.as_ref().into(), this_pos);
                let rec = match bincode::serialize(&folder_rec) {
                    Err(e) => return transaction::abort(Error::from(e)),
                    Ok(res) => res,
                };

                pos_folder.insert(path.as_bytes(), rec)?;
                pos_latest.insert(group.as_ref(), path.as_bytes())?;
                Ok(())
            })
            .map_err(|e| Error::from(e))
    }

    pub fn get_position<S, P>(&self, group: S, folder: Option<P>) -> Option<Position>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        (&self.pos_latest, &self.pos_folder)
            .transaction(|(pos_latest, pos_folder)| {
                let fld = match folder.as_ref().map(|f| f.as_ref().to_string()).or_else(|| {
                    pos_latest
                        .get(group.as_ref())
                        .map_err(|e| error!("Get last pos db error: {}", e))
                        .ok()
                        .flatten()
                        // it's safe because we know for sure we inserted string here
                        .map(|data| unsafe { String::from_utf8_unchecked(data.as_ref().into()) })
                }) {
                    Some(s) => s,
                    None => return Ok(None),
                };

                Ok(pos_folder
                    .get(&fld)
                    .map_err(|e| error!("Error reading position folder record in db: {}", e))
                    .ok()
                    .flatten()
                    .and_then(|r| {
                        bincode::deserialize::<PositionRecord>(&r)
                            .map_err(|e| error!("Error deserializing position record {}", e))
                            .ok()
                    })
                    .and_then(|m| m.get(group.as_ref()).map(|p| p.into_position(fld, 0))))
            })
            .map_err(|e: TransactionError<Error>| error!("Db transaction error: {}", e))
            .ok()
            .flatten()
    }

    fn proceed_update(&self, update: UpdateAction) {
        debug!("Update action: {:?}", update);
    }

    fn proceed_event(&self, evt: DebouncedEvent) {
        let snd = |a| {
            self.update_sender
                .send(Some(a))
                .map_err(|e| error!("Error sending update {}", e))
                .ok()
                .unwrap_or(())
        };
        match evt {
            DebouncedEvent::Create(p) => {
                let col_path = self.strip_base(&p);
                if self.is_dir(&p) {
                    snd(UpdateAction::RefreshFolder(col_path.into()));
                    snd(UpdateAction::RefreshFolder(parent_path(col_path)));
                } else {
                    snd(UpdateAction::RefreshFolder(parent_path(col_path)));
                }
            }
            DebouncedEvent::Write(p) => {
                let col_path = self.strip_base(&p);
                // TODO - check can get Write on directory?
                if self.is_dir(&p) {
                    // should be single file folder
                    snd(UpdateAction::RefreshFolder(col_path.into()));
                } else {
                    snd(UpdateAction::RefreshFolder(parent_path(col_path)));
                }
            }
            DebouncedEvent::Remove(p) => {
                let col_path = self.strip_base(&p);
                if self.is_dir(&p) {
                    snd(UpdateAction::RemoveFolder(col_path.into()));
                } else {
                    snd(UpdateAction::RefreshFolder(parent_path(col_path)))
                }
            }
            DebouncedEvent::Rename(p1, p2) => {
                let col_path = self.strip_base(&p1);
                match (p2.starts_with(&self.base_dir), self.is_dir(&p1)) {
                    (true, true) => snd(UpdateAction::RenameFolder {
                        from: col_path.into(),
                        to: self.strip_base(&p2).into(),
                    }),
                    (true, false) => snd(UpdateAction::RefreshFolder(parent_path(col_path))),
                    (false, true) => snd(UpdateAction::RemoveFolder(col_path.into())),
                    (false, false) => snd(UpdateAction::RefreshFolder(parent_path(col_path))),
                }
            }
            other => {
                error!("This event {:?} should not get here", other);
                return;
            }
        };
    }

    /// must be used only on paths with this collection
    fn strip_base<'a, P>(&self, path: &'a P) -> &'a Path
    where
        P: AsRef<Path>,
    {
        path.as_ref().strip_prefix(&self.base_dir).unwrap() // Should be safe as is used only with this collection
    }

    /// only for absolute paths
    fn is_dir<P: AsRef<Path>>(&self, path: P) -> bool {
        let path: &Path = path.as_ref();
        assert!(path.is_absolute());
        if path.metadata().map(|m| m.is_dir()).unwrap_or(false) {
            true
        } else {
            let col_path = self.strip_base(&path); // Should be safe as is used only with this collection
            if col_path
                .to_str()
                .and_then(|p| self.db.contains_key(p.as_bytes()).ok())
                .unwrap_or(false)
            {
                // it has been identified as directory before
                true
            } else {
                // have to check hard way - what if we created new .m4b in folder?
                // and we have problem with concept of single .m4b in directory !
                // TODO: implement solid logic here
                false
            }
        }
    }
}

fn parent_path<P: AsRef<Path>>(path: P) -> PathBuf {
    path.as_ref()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default()
}

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
            inner: Arc::new(CacheInner {
                pos_latest: db.open_tree("pos_latest")?,
                pos_folder: db.open_tree("pos_folder")?,
                db,
                lister,
                base_dir: root_path,
                update_sender: update_sender.clone(),
            }),
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

    pub fn run_update_loop(&mut self) {
        let update_receiver = self.update_receiver.take().expect("run multiple times");
        let inner = self.inner.clone();
        let ongoing_updater = OngoingUpdater::new(update_receiver, inner.clone());
        self.thread_updater = Some(thread::spawn(move || ongoing_updater.run()));
        let cond = self.cond.clone();

        let thread = thread::spawn(move || {
            let root_path = inner.base_dir.as_path();
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
                let mut updater = InitialUpdater::new(inner.clone());
                updater.process();

                // Notify about finish of initial scan
                {
                    let mut started = cond_mtx.lock().unwrap();
                    *started = true;
                    cond_var.notify_all();
                }

                info!("Initial scan for collection {:?} finished", inner.base_dir);

                // now update changed directories
                loop {
                    match rx.recv() {
                        Ok(event) => {
                            debug!("Change in collection {:?} => {:?}", root_path, event);
                            let interesting_event = match event {
                                DebouncedEvent::NoticeWrite(_) => continue,
                                DebouncedEvent::NoticeRemove(_) => continue,
                                evt @ DebouncedEvent::Create(_) => evt,
                                evt @ DebouncedEvent::Write(_) => evt,
                                DebouncedEvent::Chmod(_) => continue,
                                evt @ DebouncedEvent::Remove(_) => evt,
                                evt @ DebouncedEvent::Rename(_, _) => evt,
                                DebouncedEvent::Rescan => {
                                    warn!("Rescaning of collection required");
                                    break;
                                }
                                DebouncedEvent::Error(e, p) => {
                                    error!("Watch event error {} on {:?}", e, p);
                                    continue;
                                }
                            };
                            inner.proceed_event(interesting_event)
                        }
                        Err(e) => {
                            error!("Error in collection watcher channel: {}", e);
                            thread::sleep(Duration::from_secs(10));
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
        self.inner.force_update(dir_path)
    }

    pub fn flush(&self) -> Result<()> {
        let mut res = vec![];
        res.push(self.inner.db.flush());
        res.push(self.inner.pos_folder.flush());
        res.push(self.inner.pos_latest.flush());

        res.into_iter()
            .find(|r| r.is_err())
            .unwrap_or(Ok(0))
            .map(|_| ())
            .map_err(Error::from)
    }

    pub fn search<S: AsRef<str>>(&self, q: S) -> Search {
        let tokens: Vec<String> = q
            .as_ref()
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();
        let iter = self.inner.db.iter();
        Search {
            tokens,
            iter,
            prev_match: None,
        }
    }

    pub fn recent(&self, limit: usize) -> Vec<AudioFolderShort> {
        let mut heap = BinaryHeap::with_capacity(limit + 1);

        for (key, val) in self.inner.db.iter().skip(1).filter_map(|r| r.ok()) {
            let sf = kv_to_audiofolder(std::str::from_utf8(&key).unwrap(), val);
            heap.push(FolderByModification(sf));
            if heap.len() > limit {
                heap.pop();
            }
        }
        heap.into_sorted_vec().into_iter().map(|i| i.0).collect()
    }

    // positions

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

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
enum UpdateAction {
    RefreshFolder(PathBuf),
    RemoveFolder(PathBuf),
    RenameFolder { from: PathBuf, to: PathBuf },
}

impl AsRef<Path> for UpdateAction {
    fn as_ref(&self) -> &Path {
        match self {
            UpdateAction::RefreshFolder(folder) => folder.as_path(),
            UpdateAction::RemoveFolder(folder) => folder.as_path(),
            UpdateAction::RenameFolder { from, .. } => from.as_path(),
        }
    }
}

// positions
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

fn split_path<S: AsRef<str>>(p: &S) -> (String, String) {
    let s = p.as_ref();
    match s.rsplit_once('/') {
        Some((path, file)) => (path.into(), file.into()),
        None => ("".into(), s.to_owned()),
    }
}

#[derive(PartialEq, Eq, Ord)]
struct FolderByModification(AudioFolderShort);

impl PartialOrd for FolderByModification {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match other.0.modified.partial_cmp(&self.0.modified) {
            Some(Ordering::Equal) => self.0.partial_cmp(&other.0),
            other => other,
        }
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

struct OngoingUpdater {
    queue: Receiver<Option<UpdateAction>>,
    inner: Arc<CacheInner>,
    pending: HashMap<UpdateAction, SystemTime>,
    interval: Duration,
}

impl OngoingUpdater {
    fn new(queue: Receiver<Option<UpdateAction>>, inner: Arc<CacheInner>) -> Self {
        OngoingUpdater {
            queue,
            inner,
            pending: HashMap::new(),
            interval: Duration::from_secs(10),
        }
    }

    fn finish(self) {
        let inner = self.inner;
        self.pending
            .into_iter()
            .for_each(|(a, _)| inner.proceed_update(a))
    }

    fn run(mut self) {
        loop {
            match self.queue.recv_timeout(self.interval) {
                Ok(Some(action)) => {
                    self.pending.insert(action, SystemTime::now());
                }
                Ok(None) => {
                    self.finish();
                    return;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    error!("OngoingUpdater channel disconnected preliminary");
                    self.finish();
                    return;
                }
                Err(RecvTimeoutError::Timeout) => (), // just give chance to send pending actions
            }
            let current_time = SystemTime::now();
            // TODO replace with drain_filter when becomes stable
            self.pending
                .iter()
                .filter(|(_, time)| current_time.duration_since(**time).unwrap() > self.interval)
                .map(|v| v.0.clone())
                .collect::<Vec<_>>()
                .into_iter()
                .for_each(|a| {
                    self.pending.remove(&a);
                    self.inner.proceed_update(a)
                })
        }
    }
}

struct InitialUpdater {
    queue: VecDeque<AudioFolderShort>,
    inner: Arc<CacheInner>,
}

impl InitialUpdater {
    fn new(inner: Arc<CacheInner>) -> Self {
        let root = AudioFolderShort {
            name: "root".into(),
            path: Path::new("").into(),
            is_file: false,
            modified: None,
        };
        let mut queue = VecDeque::new();
        queue.push_back(root);
        InitialUpdater { queue, inner }
    }

    fn process(&mut self) {
        while let Some(folder_info) = self.queue.pop_front() {
            // process AF
            let full_path = self.inner.base_dir.join(&folder_info.path);
            let mod_ts = full_path.metadata().ok().and_then(|m| m.modified().ok());
            match self.inner.get_if_actual(&folder_info.path, mod_ts) {
                None => match self.inner.lister.list_dir(
                    &self.inner.base_dir,
                    &folder_info.path,
                    FoldersOrdering::Alphabetical,
                ) {
                    Ok(af) => {
                        self.queue.extend(af.subfolders.iter().cloned());
                        self.inner
                            .update(&folder_info.path, af)
                            .map_err(|e| error!("Cannot insert to db {}", e))
                            .ok();
                    }
                    Err(e) => error!(
                        "Cannot listing audio folder {:?}, error {}",
                        folder_info.path, e
                    ),
                },
                Some(af) => {
                    debug!("For path {:?} using cached data", folder_info.path);
                    self.queue.extend(af.subfolders)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use std::fs;

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

    #[test]
    fn test_parent_path() {
        let p1 = Path::new("usak/kulisak");
        assert_eq!(Path::new("usak"), parent_path(p1));
        let p2 = Path::new("usak");
        assert_eq!(Path::new(""), parent_path(p2));
    }
}
