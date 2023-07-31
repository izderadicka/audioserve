use std::{
    ops::Deref,
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde_json::Value;
use sled::{
    transaction::{self, TransactionError, Transactional},
    Batch, Db, IVec, Tree,
};

use crate::{
    audio_folder::{DirType, FolderLister},
    audio_meta::{AudioFolder, TimeStamp},
    cache::{
        update::{RecursiveUpdater, UpdateActionKind},
        util::{split_path, update_path},
    },
    common::PositionsData,
    error::{Error, Result},
    position::{PositionItem, PositionRecord, PositionsCollector, MAX_GROUPS},
    util::{get_file_name, get_modified},
    AudioFolderShort, FoldersOrdering, Position,
};

use super::{
    update::UpdateAction,
    util::{deser_audiofolder, parent_path},
};

#[derive(Clone)]
pub(crate) struct CacheInner {
    db: Db,
    pos_latest: Tree,
    pos_folder: Tree,
    lister: FolderLister,
    base_dir: PathBuf,
    time_to_folder_end: u32,
}

impl CacheInner {
    pub(crate) fn new(
        db: Db,

        lister: FolderLister,
        base_dir: PathBuf,
        time_to_folder_end: u32,
    ) -> Result<Self> {
        let pos_latest = db.open_tree("pos_latest")?;
        let pos_folder = db.open_tree("pos_folder")?;
        Ok(CacheInner {
            db,
            pos_latest,
            pos_folder,
            lister,
            base_dir,
            time_to_folder_end,
        })
    }
}

// access methods
impl CacheInner {
    pub(crate) fn base_dir(&self) -> &Path {
        self.base_dir.as_path()
    }

    pub(crate) fn list_dir<P: AsRef<Path>>(
        &self,
        dir_path: P,
        ordering: FoldersOrdering,
    ) -> Result<AudioFolder> {
        self.lister
            .list_dir(&self.base_dir, dir_path, ordering)
            .map_err(Error::from)
    }

    pub(crate) fn iter_folders(&self) -> sled::Iter {
        self.db.iter()
    }

    pub(crate) fn clean_up_folders(&self) {
        for key in self.iter_folders().filter_map(|e| e.ok()).map(|(k, _)| k) {
            if let Ok(rel_path) = std::str::from_utf8(&key) {
                let full_path = self.base_dir.join(rel_path);
                if !full_path.exists() || self.lister.is_collapsable_folder(&full_path) {
                    debug!("Removing {:?} from collection cache db", full_path);
                    self.remove(rel_path)
                        .map_err(|e| error!("cannot remove record from db: {}", e))
                        .ok();
                }
            }
        }
    }
}

impl CacheInner {
    pub(crate) fn get<P: AsRef<Path>>(&self, dir: P) -> Option<AudioFolder> {
        dir.as_ref()
            .to_str()
            .and_then(|p| {
                self.db
                    .get(p)
                    .map_err(|e| error!("Cannot get record for db: {}", e))
                    .ok()
                    .flatten()
            })
            .and_then(deser_audiofolder)
    }

    pub(crate) fn has_key<P: AsRef<Path>>(&self, dir: P) -> bool {
        dir.as_ref()
            .to_str()
            .and_then(|p| self.db.contains_key(p.as_bytes()).ok())
            .unwrap_or(false)
    }

    pub(crate) fn get_if_actual<P: AsRef<Path>>(
        &self,
        dir: P,
        ts: Option<SystemTime>,
    ) -> Option<AudioFolder> {
        let af = self.get(dir);
        af.as_ref()
            .and_then(|af| af.modified)
            .and_then(|cached_time| ts.map(|actual_time| cached_time >= actual_time))
            .and_then(|actual| if actual { af } else { None })
    }

    fn get_last_file<P: AsRef<Path>>(&self, dir: P) -> Option<(String, Option<u32>)> {
        self.get(dir).and_then(|d| {
            d.files.last().and_then(|p| {
                p.path.file_name().map(|n| {
                    (
                        n.to_str().unwrap().to_owned(),
                        p.meta.as_ref().map(|m| m.duration),
                    )
                })
            })
        })
    }

    pub(crate) fn update<P: AsRef<Path>>(&self, dir: P, af: AudioFolder) -> Result<()> {
        let dir = dir.as_ref().to_str().ok_or(Error::InvalidCollectionPath)?;
        bincode::serialize(&af)
            .map_err(Error::from)
            .and_then(|data| self.db.insert(dir, data).map_err(Error::from))
            .map(|_| debug!("Cache updated for {:?}", dir))
    }

    pub(crate) fn force_update<P: AsRef<Path>>(
        &self,
        dir_path: P,
        ret: bool,
    ) -> Result<Option<AudioFolder>> {
        let af = self.lister.list_dir(
            &self.base_dir,
            dir_path.as_ref(),
            FoldersOrdering::Alphabetical,
        )?;
        let rv = if ret { Some(af.clone()) } else { None };
        self.update(dir_path, af)?;
        Ok(rv)
    }

    pub(crate) fn full_path<P: AsRef<Path>>(&self, rel_path: P) -> PathBuf {
        self.base_dir.join(rel_path.as_ref())
    }

    pub(crate) fn remove<P: AsRef<Path>>(&self, dir_path: P) -> Result<Option<IVec>> {
        let path = dir_path.as_ref().to_str().ok_or(Error::InvalidPath)?;
        self.db.remove(path).map_err(Error::from)
    }

    pub(crate) fn remove_tree<P: AsRef<Path>>(&self, dir_path: P) -> Result<()> {
        let path = dir_path.as_ref().to_str().ok_or(Error::InvalidPath)?;
        let pos_batch = self.remove_positions_batch(&dir_path)?;
        let mut batch = Batch::default();
        self.db
            .scan_prefix(path)
            .filter_map(|r| r.ok())
            .for_each(|(key, _)| batch.remove(key));
        (self.db.deref(), &self.pos_folder)
            .transaction(|(db, pos_folder)| {
                db.apply_batch(&batch)?;
                pos_folder.apply_batch(&pos_batch)?;
                Ok(())
            })
            .map_err(Error::from)
    }

    pub fn flush(&self) -> Result<()> {
        let res = vec![
            self.db.flush(),
            self.pos_folder.flush(),
            self.pos_latest.flush(),
        ];
        res.into_iter()
            .find(|r| r.is_err())
            .unwrap_or(Ok(0))
            .map(|_| ())
            .map_err(Error::from)
    }
}

// positions
impl CacheInner {
    pub(crate) fn insert_position<S, P>(
        &self,
        group: S,
        path: P,
        position: f32,
        finished: bool,
        ts: Option<TimeStamp>,
        use_ts: bool,
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        let (path, file) = split_path(&path);
        if let Some((last_file, last_file_duration)) = self.get_last_file(path) {
            (&self.pos_latest, &self.pos_folder)
                .transaction(move |(pos_latest, pos_folder)| {
                    let mut folder_rec = pos_folder
                        .get(path)
                        .map_err(|e| error!("Db get error: {}", e))
                        .ok()
                        .flatten()
                        .and_then(|data| {
                            bincode::deserialize::<PositionRecord>(&data)
                                .map_err(|e| error!("Db item deserialization error: {}", e))
                                .ok()
                        })
                        .unwrap_or_default();

                    if let Some(ts) = ts {
                        if let Some(current_record) = folder_rec.get(group.as_ref()) {
                            if current_record.timestamp > ts {
                                info!(
                                    "Position not inserted for folder {} because it's outdated. It has timestamp {:?}, but we have ts {:?}",
                                    path,
                                    ts,
                                    current_record.timestamp
                                );
                                return transaction::abort(Error::IgnoredPosition);
                            } else {
                                debug!(
                                    "Updating position record {} dated {:?} with new from {:?}",
                                    path, current_record.timestamp, ts
                                );
                            }
                        }
                    }
                    let this_pos = PositionItem {
                        folder_finished: finished
                            || (file.eq(&last_file)
                                && last_file_duration
                                    .map(|d| d.saturating_sub(position.round() as u32))
                                    .map(|dif| dif < self.time_to_folder_end)
                                    .unwrap_or(false)),
                        file: file.into(),
                        timestamp: if let (true, Some(ts)) = (use_ts, ts) {
                            ts
                        } else {
                            TimeStamp::now()
                        },
                        position,
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
                .map_err(Error::from)
        } else {
            // folder does not have playable file or does not exist in cache
            warn!(
                "Trying to insert position for unknown or empty folder {}",
                path
            );
            Err(Error::IgnoredPosition)
        }
    }

    pub(crate) fn get_position<S, P>(&self, group: S, folder: Option<P>) -> Option<Position>
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
                    .and_then(|m| m.get(group.as_ref()).map(|p| p.to_position(fld, 0))))
            })
            .map_err(|e: TransactionError<Error>| error!("Db transaction error: {}", e))
            .ok()
            .flatten()
    }

    pub(crate) fn get_positions_recursive<S, P>(
        &self,
        group: S,
        folder: P,
        collection_no: usize,
        res: &mut PositionsCollector,
    ) where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        CacheInner::positions_from_iter(
            self.pos_folder.scan_prefix(folder.as_ref()),
            group,
            collection_no,
            res,
        )
    }

    pub(crate) fn is_finished<S, P>(&self, group: S, dir: P) -> bool
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.pos_folder
            .get(dir.as_ref())
            .map_err(|e| error!("Error reading position folder record in db: {}", e))
            .ok()
            .flatten()
            .and_then(|r| {
                bincode::deserialize::<PositionRecord>(&r)
                    .map_err(|e| error!("Error deserializing position record {}", e))
                    .ok()
            })
            .and_then(|m| m.get(group.as_ref()).map(|p| p.folder_finished))
            .unwrap_or(false)
    }

    pub(crate) fn update_subfolder<S: AsRef<str>>(&self, group: S, sf: &mut AudioFolderShort) {
        sf.finished = sf
            .path
            .to_str()
            .map(|path| self.is_finished(&group, path))
            .unwrap_or(false)
    }

    pub(crate) fn update_subfolders<S: AsRef<str>>(
        &self,
        group: S,
        subfolders: &mut [AudioFolderShort],
    ) {
        subfolders
            .iter_mut()
            .for_each(|sf| self.update_subfolder(&group, sf))
    }

    fn positions_from_iter<I, S>(
        iter: I,
        group: S,
        collection_no: usize,
        res: &mut PositionsCollector,
    ) where
        I: Iterator<Item = std::result::Result<(sled::IVec, sled::IVec), sled::Error>>,
        S: AsRef<str>,
    {
        iter.filter_map(|res| {
            res.map_err(|e| error!("Error reading from positions db: {}", e))
                .ok()
                .and_then(|(folder, rec)| {
                    let rec: PositionRecord = bincode::deserialize(&rec)
                        .map_err(|e| error!("Position deserialization error: {}", e))
                        .ok()?;
                    let folder = String::from_utf8(folder.as_ref().into()).unwrap(); // known to be valid UTF8
                    rec.get(group.as_ref())
                        .map(|p| p.to_position(folder, collection_no))
                })
        })
        .for_each(|p| res.add(p))
    }

    pub(crate) fn get_all_positions_for_group<S>(
        &self,
        group: S,
        collection_no: usize,
        res: &mut PositionsCollector,
    ) where
        S: AsRef<str>,
    {
        CacheInner::positions_from_iter(self.pos_folder.iter(), group, collection_no, res)
    }

    fn remove_positions_batch<P: AsRef<Path>>(&self, path: P) -> Result<Batch> {
        let mut batch = Batch::default();
        self.pos_folder
            .scan_prefix(path.as_ref().to_str().ok_or(Error::InvalidPath)?)
            .filter_map(|r| {
                r.map_err(|e| error!("Cannot read positions db: {}", e))
                    .ok()
            })
            .for_each(|(k, _)| batch.remove(k));

        Ok(batch)
    }

    fn remove_positions<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let batch = self.remove_positions_batch(path)?;
        self.pos_folder
            .transaction(|pos_folder| pos_folder.apply_batch(&batch).map_err(|e| e.into()))
            .map_err(Error::from)
    }

    fn rename_positions(&self, from: &Path, to: &Path) -> Result<()> {
        let mut delete_batch = Batch::default();
        let mut insert_batch = Batch::default();
        let mut group_batch = Batch::default();

        let iter = self
            .pos_folder
            .scan_prefix(from.to_str().ok_or(Error::InvalidPath)?)
            .filter_map(|r| {
                r.map_err(|e| error!("Cannot read positions db: {}", e))
                    .ok()
            });
        for (k, v) in iter {
            delete_batch.remove(k.clone());
            let new_key = update_path(from, to, Path::new(std::str::from_utf8(&k)?))?;
            let new_key = new_key.to_str().unwrap();
            insert_batch.insert(new_key, v);
        }

        for (k, v) in self.pos_latest.iter().filter_map(|r| {
            r.map_err(|e| error!("Error reading latest position db: {}", e))
                .ok()
        }) {
            let fld = Path::new(std::str::from_utf8(&v)?);
            if fld.starts_with(from) {
                let new_path = update_path(from, to, fld)?;
                let new_path = new_path.to_str().unwrap(); // was created from UTF8 few lines above
                group_batch.insert(k, new_path);
            }
        }

        (&self.pos_folder, &self.pos_latest)
            .transaction(|(pos_folder, pos_latest)| {
                pos_folder.apply_batch(&delete_batch)?;
                pos_folder.apply_batch(&insert_batch)?;
                pos_latest.apply_batch(&group_batch)?;
                Ok(())
            })
            .map_err(Error::from)
    }

    pub(crate) fn clean_up_positions(&self) {
        let mut batch = Batch::default();
        self.pos_folder
            .iter()
            .filter_map(|r| match r {
                Ok((k, _)) => {
                    if !self.db.contains_key(&k).unwrap_or(false) {
                        Some(k)
                    } else {
                        None
                    }
                }
                Err(e) => {
                    error!("Error reading from db: {}", e);
                    None
                }
            })
            .for_each(|k| {
                debug!(
                    "Removing positions for directory {:?} as it does not exists",
                    std::str::from_utf8(&k)
                );
                batch.remove(k);
            });
        self.pos_folder
            .apply_batch(batch)
            .map_err(|e| error!("Cannot remove positions: {}", e))
            .ok();
    }

    pub(crate) fn write_json_positions<F: std::io::Write>(&self, mut file: &mut F) -> Result<()> {
        write!(file, "{{")?;
        for (idx, res) in self.pos_folder.iter().enumerate() {
            match res {
                Ok((k, v)) => {
                    let folder = std::str::from_utf8(&k)?;
                    let res: PositionRecord = bincode::deserialize(&v)?;
                    write!(file, "\"{}\":", folder)?;
                    serde_json::to_writer(&mut file, &res)?;
                    if idx < self.pos_folder.len() - 1 {
                        writeln!(file, ",")?;
                    } else {
                        writeln!(file)?;
                    }
                }
                Err(e) => error!("Error when reading from position db: {}", e),
            }
        }
        write!(file, "}}")?;
        Ok(())
    }

    // It may not be much efficient, but it's simple and it's ok, as restore from will be rarely used
    pub(crate) fn read_json_positions(&self, data: PositionsData) -> Result<()> {
        match data {
            PositionsData::Legacy(_) => todo!(),
            PositionsData::V1(json) => {
                for (folder, rec) in json.into_iter() {
                    if let Value::Object(map) = rec {
                        for (group, v) in map.into_iter() {
                            let item: PositionItem = serde_json::from_value(v)?;
                            let path = if folder.is_empty() {
                                item.file
                            } else {
                                folder.clone() + "/" + &item.file
                            };
                            trace!("Inserting position {} ts {:?}", path, item.timestamp);
                            self.insert_position(
                                group,
                                path,
                                item.position,
                                item.folder_finished,
                                Some(item.timestamp),
                                true,
                            )
                            .or_else(|e| {
                                if matches!(e, Error::IgnoredPosition) {
                                    Ok(())
                                } else {
                                    Err(e)
                                }
                            })?;
                        }
                    } else {
                        return Err(Error::JsonSchemaError(format!(
                            "Expected object for key {}",
                            folder
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

// Updating based on fs events
impl CacheInner {
    fn force_update_recursive<P: Into<PathBuf>>(&self, folder: P) {
        let folder = folder.into();
        let af: AudioFolderShort = AudioFolderShort {
            name: get_file_name(&folder).into(),
            modified: None,
            path: folder,
            is_file: false,
            finished: false,
        };
        let updater = RecursiveUpdater::new(self, Some(af), false);
        updater.process();
    }

    fn update_recursive_after_rename(&self, from: &Path, to: &Path) -> Result<()> {
        let mut delete_batch = Batch::default();
        let mut insert_batch = Batch::default();

        let mut updated = get_modified(self.base_dir.join(to));
        debug!("Renamed root modified for {:?}", updated);
        for item in self.db.scan_prefix(from.to_str().unwrap()) {
            // safe to unwrap as we insert only valid strings
            let (k, v) = item?;
            let mut folder_rec: AudioFolder = bincode::deserialize(&v)?;
            let p: &Path = Path::new(unsafe { std::str::from_utf8_unchecked(&k) }); // we insert only valid strings as keys
            let new_key = update_path(from, to, p)?;
            let new_key = new_key.to_str().ok_or(Error::InvalidPath)?;
            trace!(
                "Processing path {} from key {} in to {:?}",
                new_key,
                std::str::from_utf8(&k).unwrap(),
                to
            );
            if let Some(mod_time) = updated {
                if to.to_str() == Some(new_key) {
                    // this is root renamed folder, for which mod time has changed
                    folder_rec.modified = Some(mod_time.into());
                    debug!(
                        "Updating modified for {} to {:?}",
                        new_key, folder_rec.modified
                    );
                    updated.take();
                }
            }
            delete_batch.remove(k);
            for sf in folder_rec.subfolders.iter_mut() {
                let new_path = update_path(from, to, &sf.path)?;
                sf.path = new_path;
            }
            for f in folder_rec.files.iter_mut() {
                let new_path = update_path(from, to, &f.path)?;
                f.path = new_path;
            }
            if let Some(mut d) = folder_rec.description.take() {
                d.path = update_path(from, to, &d.path)?;
                folder_rec.description = Some(d);
            }

            if let Some(mut c) = folder_rec.cover.take() {
                c.path = update_path(from, to, &c.path)?;
                folder_rec.cover = Some(c);
            }

            insert_batch.insert(new_key, bincode::serialize(&folder_rec)?);
        }

        self.db
            .transaction(|db| {
                db.apply_batch(&delete_batch)?;
                db.apply_batch(&insert_batch)?;
                Ok(())
            })
            .map_err(Error::from)
    }

    pub(crate) fn proceed_update(&self, update: UpdateAction) {
        debug!("Update action: {:?}", update);
        let folder = update.path;
        match update.kind {
            UpdateActionKind::RefreshFolder => {
                self.force_update(&folder, false)
                    .map_err(|e| warn!("Error updating folder in cache: {}", e))
                    .ok();
            }
            UpdateActionKind::RefreshFolderRecursive => {
                self.force_update_recursive(folder);
            }
            UpdateActionKind::RemoveFolder => {
                self.remove_tree(&folder)
                    .map_err(|e| warn!("Error removing folder from cache: {}", e))
                    .ok();
            }
            UpdateActionKind::RenameFolder { to } => {
                let from = folder;
                if let Err(e) = self.update_recursive_after_rename(&from, &to) {
                    error!("Failed to do recursive rename, error: {}, we will have to do rescan of {:?}", e, &to);
                    self.remove_tree(&from)
                        .map_err(|e| warn!("Error removing folder from cache: {}", e))
                        .ok();
                    self.force_update_recursive(&to);
                }
                if let Err(e) = self.rename_positions(&from, &to) {
                    error!(
                        "Error when renaming positions, will try at least to delete them: {}",
                        e
                    );
                    self.remove_positions(&from)
                        .map_err(|e| error!("Even deleting positions failed: {}", e))
                        .ok();
                }
            }
        }
    }

    pub(crate) fn get_true_parent(&self, rel_path: &Path, full_path: &Path) -> PathBuf {
        let parent = parent_path(rel_path);
        if self.lister.collapse_cd_enabled()
            && full_path
                .parent()
                .map(|p| self.path_type(p).is_collapsed())
                .unwrap_or(false)
        {
            parent_path(parent)
        } else {
            parent
        }
    }

    /// must be used only on paths with this collection
    pub(crate) fn strip_base<'a, P>(&self, full_path: &'a P) -> &'a Path
    where
        P: AsRef<Path>,
    {
        full_path.as_ref().strip_prefix(&self.base_dir).unwrap() // Should be safe as is used only with this collection
    }

    /// only for absolute paths
    pub(crate) fn path_type<P: AsRef<Path>>(&self, full_path: P) -> FolderType {
        let full_path: &Path = full_path.as_ref();
        assert!(full_path.is_absolute(), "path {:?}", full_path);
        let col_path = self.strip_base(&full_path);
        match self.lister.get_dir_type(full_path) {
            Ok(DirType::Dir) => {
                if self.has_key(col_path) {
                    FolderType::RegularDir
                } else if self.lister.is_collapsable_folder(col_path) {
                    FolderType::CollapsedDir
                } else {
                    FolderType::NewDir
                }
            }
            Ok(DirType::File { .. }) => {
                if self.has_key(col_path) {
                    FolderType::FileDir
                } else {
                    FolderType::NewFileDir
                }
            }
            Ok(DirType::Other) => FolderType::RegularFile,
            Err(e) => {
                if !matches!(e.kind(), std::io::ErrorKind::NotFound) {
                    error!("Error determining dir type: {}", e);
                    FolderType::Unknown
                } else if self.has_key(col_path) {
                    FolderType::DeletedDir
                } else {
                    FolderType::Unknown
                }
            }
        }
    }

    pub(crate) fn is_collapsable_folder(&self, p: impl AsRef<Path>) -> bool {
        self.lister.is_collapsable_folder(p)
    }
}

pub enum FolderType {
    RegularDir,
    DeletedDir,
    FileDir,
    RegularFile,
    CollapsedDir,
    NewDir,
    NewFileDir,
    Unknown,
}

impl FolderType {
    pub fn is_dir(&self) -> bool {
        use FolderType::*;
        !matches!(self, RegularFile | Unknown | CollapsedDir)
    }

    pub fn is_collapsed(&self) -> bool {
        matches!(self, FolderType::CollapsedDir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trailing_slash() {
        let p1 = Path::new("kulisak");
        let p2 = p1.join("");
        assert_eq!(p1, p2);
        assert_ne!(p1.to_str(), p2.to_str());
        assert_eq!(p2.to_str().unwrap(), "kulisak/");
    }
}
