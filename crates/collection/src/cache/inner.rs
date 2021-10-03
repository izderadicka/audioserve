use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

use crossbeam_channel::Sender;
use notify::DebouncedEvent;
use sled::{
    transaction::{self, TransactionError, Transactional},
    Batch, Db, IVec, Tree,
};

use crate::{
    audio_folder::FolderLister,
    audio_meta::{AudioFolder, TimeStamp},
    cache::util::split_path,
    error::{Error, Result},
    position::{PositionItem, PositionRecord, MAX_GROUPS},
    util::get_meta,
    FoldersOrdering, Position,
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
    update_sender: Sender<Option<UpdateAction>>,
}

impl CacheInner {
    pub(crate) fn new(
        db: Db,

        lister: FolderLister,
        base_dir: PathBuf,
        update_sender: Sender<Option<UpdateAction>>,
    ) -> Result<Self> {
        let pos_latest = db.open_tree("pos_latest")?;
        let pos_folder = db.open_tree("pos_folder")?;
        Ok(CacheInner {
            db,
            pos_latest,
            pos_folder,
            lister,
            base_dir,
            update_sender,
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

    pub(crate) fn update<P: AsRef<Path>>(&self, dir: P, af: AudioFolder) -> Result<()> {
        let dir = dir
            .as_ref()
            .to_str()
            .ok_or_else(|| Error::InvalidCollectionPath)?;
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
        let path = dir_path.as_ref().to_str().ok_or(Error::InvalidFileName)?;
        self.db.remove(path).map_err(Error::from)
    }

    pub(crate) fn remove_tree<P: AsRef<Path>>(&self, dir_path: P) -> Result<()> {
        let path = dir_path.as_ref().to_str().ok_or(Error::InvalidFileName)?;
        let mut batch = Batch::default();
        self.db
            .scan_prefix(path)
            .filter_map(|r| r.ok())
            .for_each(|(key, _)| batch.remove(key));
        self.db.apply_batch(batch).map_err(Error::from)
    }

    pub fn flush(&self) -> Result<()> {
        let mut res = vec![];
        res.push(self.db.flush());
        res.push(self.pos_folder.flush());
        res.push(self.pos_latest.flush());

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
                    .and_then(|m| m.get(group.as_ref()).map(|p| p.into_position(fld, 0))))
            })
            .map_err(|e: TransactionError<Error>| error!("Db transaction error: {}", e))
            .ok()
            .flatten()
    }
}

// Updating based on fs events
impl CacheInner {
    pub(crate) fn proceed_update(&self, update: UpdateAction) {
        debug!("Update action: {:?}", update);
        match update {
            UpdateAction::RefreshFolder(folder) => {
                self.force_update(&folder, false)
                    .map_err(|e| error!("Error updating folder: {}", e))
                    .ok();
            }
            UpdateAction::RemoveFolder(folder) => {
                self.remove_tree(&folder)
                    .map_err(|e| error!("Error deleting folder: {}", e))
                    .ok();
                self.force_update(parent_path(&folder), false).ok();
                //TODO: need also to remove positions
            }
            UpdateAction::RenameFolder { from, to } => {
                self.remove_tree(&from)
                    .map_err(|e| error!("Error deleting folder: {}", e))
                    .ok();
                let orig_parent = parent_path(&from);
                let new_parent = parent_path(&to);
                self.force_update(&orig_parent, false).ok();
                if new_parent != orig_parent {
                    self.force_update(&new_parent, false).ok();
                }
                self.force_update(&to, false).ok();
                // TODO: we need to update all subfolders new location

                // TODO: need also to move positions
            }
        }
    }

    pub(crate) fn proceed_event(&self, evt: DebouncedEvent) {
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
        if get_meta(path).map(|m| m.is_dir()).unwrap_or(false) {
            true
        } else {
            let col_path = self.strip_base(&path);
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
