#[macro_use]
extern crate log;

pub use audio_folder::{list_dir_files_only, list_dir_files_with_subdirs, parse_chapter_path};
pub use audio_meta::{
    extract_cover, extract_description, init_media_lib, AudioFile, AudioFolderShort,
    FoldersOrdering, TimeSpan,
};
use audio_meta::{AudioFolder, TimeStamp};
use cache::CollectionCache;
use common::{Collection, CollectionTrait, PositionsTrait};
pub use common::{CollectionOptions, CollectionOptionsMap};
use error::{Error, Result};
use legacy_pos::LegacyPositions;
pub use media_info::tags;
use no_cache::CollectionDirect;
pub use position::{Position, PositionFilter};
use serde_json::{Map, Value};
#[cfg(feature = "async")]
use std::sync::Arc;
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    thread::JoinHandle,
};
pub use util::guess_mime_type;

use crate::{
    common::PositionsData,
    position::{PositionItem, PositionsCollector},
};

pub mod audio_folder;
pub mod audio_meta;
pub mod cache;
pub(crate) mod collator;
pub mod common;
pub mod error;
mod legacy_pos;
pub(crate) mod no_cache;
pub mod position;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAX_POSITIONS: usize = 1_000;
pub use common::MINIMUM_CHAPTER_DURATION;

fn check_version<P: AsRef<Path>>(db_dir: P) -> Result<()> {
    let db_dir = db_dir.as_ref();
    let version_file = db_dir.join(".version");
    if version_file.exists() {
        let mut col_db_version = String::new();
        File::open(&version_file)?.read_to_string(&mut col_db_version)?;
        if col_db_version != VERSION {
            warn!(
                "Your collection cache {:?} is version {}, which is different from current {}, \
            if experiencing problems force full reload, in worst case delete it and restore positions from backup. \
            You can delete {:?}, if everything is OK, and warning will disapear till next version change",
                db_dir, col_db_version, VERSION, version_file
            );
        }
    } else {
        if !db_dir.exists() {
            std::fs::create_dir_all(db_dir)?
        }
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(version_file)?;
        f.write_all(VERSION.as_bytes())?;
    }

    Ok(())
}

pub struct Collections {
    caches: Vec<Collection>,
}

impl Collections {
    pub fn new_with_detail<I, P1, P2>(
        collections_dirs: Vec<PathBuf>,
        mut collections_options: CollectionOptionsMap,
        db_path: P2,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = P1>,
        P1: Into<PathBuf>,
        P2: AsRef<Path>,
    {
        check_version(&db_path)?;
        let db_path = db_path.as_ref();
        let caches = collections_dirs
            .into_iter()
            .map(move |collection_path| {
                let opt = collections_options.get_col_options(&collection_path);
                if opt.no_cache {
                    info!("Collection {:?} is not using cache", collection_path);
                    Ok(CollectionDirect::new(collection_path, opt).into())
                } else {
                    CollectionCache::new(collection_path, db_path, opt)
                        .map(|mut cache| {
                            cache.run_update_loop();
                            cache
                        })
                        .map(Collection::from)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Collections { caches })
    }
}

impl Collections {
    fn get_cache(&self, collection: usize) -> Result<&Collection> {
        self.caches
            .get(collection)
            .ok_or(Error::MissingCollectionCache(collection))
    }

    pub fn list_dir<P: AsRef<Path>>(
        &self,
        collection: usize,
        dir_path: P,
        ordering: FoldersOrdering,
        group: Option<String>,
    ) -> Result<AudioFolder> {
        self.get_cache(collection)?
            .list_dir(dir_path, ordering, group)
    }

    pub fn get_folder_cover_path(
        &self,
        collection: usize,
        dir_path: impl AsRef<Path>,
    ) -> Result<Option<PathBuf>> {
        let col = self.get_cache(collection)?;
        col.get_folder_cover_path(dir_path)
            .map(|p| p.map(|p| col.base_dir().join(&p)))
    }

    pub fn flush(&self) -> Result<()> {
        let mut result = vec![];
        for c in &self.caches {
            result.push(c.flush())
        }
        result.into_iter().find(|r| r.is_err()).unwrap_or(Ok(()))
    }

    pub fn search<S: AsRef<str>>(
        &self,
        collection: usize,
        q: S,
        ordering: FoldersOrdering,
        group: Option<String>,
    ) -> Result<Vec<AudioFolderShort>> {
        let mut res = self.get_cache(collection)?.search(q, group);

        res.sort_unstable_by(|a, b| a.compare_as(ordering, b));
        Ok(res)
    }

    pub fn recent(
        &self,
        collection: usize,
        limit: usize,
        group: Option<String>,
    ) -> Result<Vec<AudioFolderShort>> {
        self.get_cache(collection)
            .map(|cache| cache.recent(limit, group))
    }
}

// positions
impl Collections {
    pub fn insert_position<S, P>(
        &self,
        collection: usize,
        group: S,
        path: P,
        position: f32,
        folder_finished: bool,
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.get_cache(collection)?
            .insert_position(group, path, position, folder_finished, None)
    }

    pub fn insert_position_if_newer<S, P>(
        &self,
        collection: usize,
        group: S,
        path: P,
        position: f32,
        folder_finished: bool,
        ts: TimeStamp,
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.get_cache(collection)?.insert_position(
            group,
            path,
            position,
            folder_finished,
            Some(ts),
        )
    }

    pub fn get_position<S, P>(&self, collection: usize, group: S, folder: P) -> Option<Position>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.get_cache(collection)
            .map_err(|e| error!("get position for invalid collection: {}", e))
            .ok()
            .and_then(|c| c.get_position(group, Some(folder)))
            .map(|mut p| {
                p.collection = collection;
                p
            })
    }

    pub fn get_last_position<S, P>(&self, group: S) -> Option<Position>
    where
        S: AsRef<str>,
    {
        let mut res = None;
        for c in 0..self.caches.len() {
            let cache = self.get_cache(c).expect("cache availavle"); // is safe, because we are iterating over known range
            let pos = cache.get_position::<_, String>(&group, None).map(|mut p| {
                p.collection = c;
                p
            });
            match (&mut res, pos) {
                (None, Some(p)) => res = Some(p),
                (Some(ref prev), Some(p)) => {
                    if p.timestamp > prev.timestamp {
                        res = Some(p)
                    }
                }
                (_, None) => (),
            }
        }
        res
    }

    pub fn force_rescan(self: std::sync::Arc<Self>) {
        self.caches.iter().for_each(|c| c.signal_rescan())
    }

    pub fn backup_positions<P: Into<PathBuf>>(&self, backup_file: P) -> Result<()> {
        let fname: PathBuf = backup_file.into();
        let mut f = std::fs::File::create(fname)?;
        write!(f, "{{")?;
        for (idx, c) in self.caches.iter().enumerate() {
            write!(
                f,
                "\"{}\":",
                c.base_dir().to_str().ok_or(Error::InvalidPath)?
            )?;
            c.write_json_positions(&mut f)?;
            if idx < self.caches.len() - 1 {
                writeln!(f, ",")?;
            } else {
                writeln!(f)?;
            }
        }
        write!(f, "}}")?;
        Ok(())
    }

    pub fn restore_positions<P2, P3>(
        collections_dirs: Vec<PathBuf>,
        collections_options: CollectionOptionsMap,
        db_path: P2,
        backup_file: BackupFile<P3>,
    ) -> Result<()>
    where
        P2: AsRef<Path>,
        P3: AsRef<Path>,
    {
        check_version(&db_path)?;
        let threads = match backup_file {
            BackupFile::V1(backup_file) => Collections::restore_positions_v1(
                collections_dirs,
                collections_options,
                db_path,
                backup_file,
            ),
            BackupFile::Legacy(backup_file) => Collections::restore_positions_legacy(
                collections_dirs,
                collections_options,
                db_path,
                backup_file,
            ),
        }?;

        threads.into_iter().for_each(|t| {
            t.join()
                .map_err(|_| error!("Positions restore thread failed"))
                .ok();
        });

        Ok(())
    }

    fn restore_positions_v1<P2, P3>(
        collections_dirs: Vec<PathBuf>,
        mut collections_options: CollectionOptionsMap,
        db_path: P2,
        backup_file: P3,
    ) -> Result<Vec<JoinHandle<()>>>
    where
        P2: AsRef<Path>,
        P3: AsRef<Path>,
    {
        let db_path = db_path.as_ref();
        let mut data: Map<String, Value> =
            serde_json::from_reader(std::io::BufReader::new(File::open(backup_file)?))?;

        let threads = collections_dirs
            .into_iter()
            .filter_map(move |collection_path| {
                let opt = collections_options.get_col_options(&collection_path);
                if !opt.no_cache {
                    collection_path
                        .to_str()
                        .and_then(|path| data.remove(path))
                        .and_then(|v| {
                            if let Value::Object(v) = v {
                                CollectionCache::restore_positions(
                                    collection_path.clone(),
                                    db_path,
                                    opt,
                                    PositionsData::V1(v),
                                )
                                .map_err(|e| {
                                    error!("Failed to restore positions from backup: {}", e)
                                })
                                .ok()
                            } else {
                                None
                            }
                        })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        Ok(threads)
    }

    fn restore_positions_legacy<P2, P3>(
        collections_dirs: Vec<PathBuf>,
        mut collections_options: CollectionOptionsMap,
        db_path: P2,
        backup_file: P3,
    ) -> Result<Vec<JoinHandle<()>>>
    where
        P2: AsRef<Path>,
        P3: AsRef<Path>,
    {
        let db_path = db_path.as_ref();
        let data: LegacyPositions =
            serde_json::from_reader(std::io::BufReader::new(File::open(backup_file)?))?;

        let mut col_positions: HashMap<usize, HashMap<String, HashMap<String, PositionItem>>> =
            HashMap::new();
        for (group, m) in data.table.into_iter() {
            //HACK: handle error in clent, which caused invalid positions to be inserted
            if group.starts_with("null") {
                continue;
            }
            for (col_path, pos) in m.into_iter() {
                let (col_no, path) = col_path.split_once('/').unwrap_or((col_path.as_str(), ""));

                let col_no: usize = col_no.parse().map_err(|_| {
                    Error::JsonDataError(format!(
                        "Collection {} in {} is not number",
                        col_no, col_path
                    ))
                })?;
                let path = path.to_string();
                let item = PositionItem {
                    file: pos.file,
                    position: pos.position,
                    timestamp: pos.timestamp.into(),
                    folder_finished: false,
                };

                col_positions
                    .entry(col_no)
                    .or_default()
                    .entry(path)
                    .or_default()
                    .entry(group.clone())
                    .and_modify(|e| {
                        if item.timestamp > e.timestamp {
                            *e = item.clone();
                        }
                    })
                    .or_insert(item);
            }
        }

        let threads = collections_dirs
            .into_iter()
            .enumerate()
            .filter_map(move |(col_no, collection_path)| {
                let opt = collections_options.get_col_options(&collection_path);
                if !opt.no_cache {
                    col_positions.remove(&col_no).and_then(|v| {
                        // HACK: This is just dirty trick to get same structure as for current positions JSON
                        //    but I hope it did not mind, because it's just migration function to be used once
                        let json_data =
                            serde_json::to_string(&v).expect("Serialization should not fail");
                        let json: Map<String, Value> = serde_json::from_str(&json_data)
                            .expect("Deserialiation should not fail");

                        CollectionCache::restore_positions(
                            collection_path.clone(),
                            db_path,
                            opt,
                            PositionsData::V1(json),
                        )
                        .map_err(|e| error!("Failed to restore positions from backup: {}", e))
                        .ok()
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        Ok(threads)
    }
}

pub enum BackupFile<P>
where
    P: AsRef<Path>,
{
    V1(P),
    Legacy(P),
}

#[cfg(feature = "async")]
macro_rules! spawn_blocking {
    ($block:block) => {
        tokio::task::spawn_blocking(move || $block).await
    };
}

// positions async
#[cfg(feature = "async")]
impl Collections {
    pub async fn insert_position_async<S, P>(
        self: Arc<Self>,
        collection: usize,
        group: S,
        path: P,
        position: f32,
        folder_finished: bool,
    ) -> Result<()>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        spawn_blocking!({
            self.get_cache(collection)?.insert_position(
                group,
                path,
                position,
                folder_finished,
                None,
            )
        })
        .unwrap_or_else(|e| Err(Error::from(e)))
    }

    pub async fn get_folder_cover_path_async<P>(
        self: Arc<Self>,
        collection: usize,
        dir_path: P,
    ) -> Result<Option<PathBuf>>
    where
        P: AsRef<Path> + Send + 'static,
    {
        spawn_blocking!({ self.get_folder_cover_path(collection, dir_path) })
            .unwrap_or_else(|e| Err(Error::from(e)))
    }

    pub async fn insert_position_if_newer_async<S, P>(
        self: Arc<Self>,
        collection: usize,
        group: S,
        path: P,
        position: f32,
        folder_finished: bool,
        ts: TimeStamp,
    ) -> Result<()>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        spawn_blocking!({
            self.get_cache(collection)?.insert_position(
                group,
                path,
                position,
                folder_finished,
                Some(ts),
            )
        })
        .unwrap_or_else(|e| Err(Error::from(e)))
    }

    pub async fn get_positions_recursive_async<S, P>(
        self: Arc<Self>,
        collection: usize,
        group: S,
        folder: P,
        filter: Option<PositionFilter>,
    ) -> Vec<Position>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        spawn_blocking!({
            let mut res = PositionsCollector::with_optional_filter(
                MAX_POSITIONS,
                filter.and_then(|f| f.into_option()),
            );
            if let Ok(c) = self
                .get_cache(collection)
                .map_err(|e| error!("Invalid collection used in get_position: {}", e))
            {
                c.get_positions_recursive(group, folder, collection, &mut res)
            };
            res.into_vec()
        })
        .unwrap_or_else(|e| {
            error!("Task join error: {}", e);
            vec![]
        })
    }

    pub async fn get_position_async<S, P>(
        self: Arc<Self>,
        collection: usize,
        group: S,
        folder: P,
    ) -> Option<Position>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        spawn_blocking!({
            self.get_cache(collection)
                .map_err(|e| error!("Invalid collection used in get_position: {}", e))
                .ok()
                .and_then(|c| {
                    c.get_position(group, Some(folder)).map(|mut p| {
                        p.collection = collection;
                        p
                    })
                })
        })
        .unwrap_or_else(|e| {
            error!("Task join error: {}", e);
            None
        })
    }

    pub async fn get_all_positions_for_group_async<S>(
        self: Arc<Self>,
        group: S,
        filter: Option<PositionFilter>,
    ) -> Vec<Position>
    where
        S: AsRef<str> + Send + Clone + 'static,
    {
        spawn_blocking!({
            let mut res = PositionsCollector::with_optional_filter(
                MAX_POSITIONS,
                filter.and_then(|f| f.into_option()),
            );
            for (cn, c) in self.caches.iter().enumerate() {
                c.get_all_positions_for_group(group.clone(), cn, &mut res);
            }
            res.into_vec()
        })
        .unwrap_or_else(|e| {
            error!("Task join error: {}", e);
            vec![]
        })
    }

    pub async fn get_last_position_async<S>(self: Arc<Self>, group: S) -> Option<Position>
    where
        S: AsRef<str> + Send + 'static,
    {
        spawn_blocking!({
            let mut res = None;
            for c in 0..self.caches.len() {
                let cache = self.get_cache(c).expect("cache available"); // is safe, because we are iterating over known range
                let g: String = group.as_ref().to_owned();
                let pos = cache.get_position::<_, String>(g, None).map(|mut p| {
                    p.collection = c;
                    p
                });
                match (&mut res, pos) {
                    (None, Some(p)) => res = Some(p),
                    (Some(ref prev), Some(p)) => {
                        if p.timestamp > prev.timestamp {
                            res = Some(p)
                        }
                    }
                    (_, None) => (),
                }
            }
            res
        })
        .unwrap_or_else(|e| {
            error!("Task join error: {}", e);
            None
        })
    }

    pub async fn backup_positions_async<P>(self: Arc<Self>, backup_file: P) -> Result<()>
    where
        P: Into<PathBuf> + Send + 'static,
    {
        spawn_blocking!({ self.backup_positions(backup_file) })
            .unwrap_or_else(|e| Err(Error::from(e)))
    }
}
