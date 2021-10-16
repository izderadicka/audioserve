#[macro_use]
extern crate log;

use audio_folder::{FolderLister, FoldersOptions};
use audio_meta::{AudioFolder, TimeStamp};
use cache::CollectionCache;
use common::{Collection, CollectionOptions, CollectionTrait, PositionsTrait};
use error::{Error, Result};
use no_cache::CollectionDirect;
#[cfg(feature = "async")]
use std::sync::Arc;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub use audio_folder::{list_dir_files_only, parse_chapter_path};
pub use audio_meta::{init_media_lib, AudioFile, AudioFolderShort, FoldersOrdering, TimeSpan};
pub use position::Position;
pub use util::guess_mime_type;

pub mod audio_folder;
pub mod audio_meta;
mod cache;
pub mod common;
pub mod error;
pub(crate) mod no_cache;
pub mod position;
pub mod util;

pub struct Collections {
    caches: Vec<Collection>,
}

impl Collections {
    pub fn new_with_detail<I, P1, P2>(
        collections_dirs: Vec<PathBuf>,
        collections_options: HashMap<PathBuf, CollectionOptions>,
        db_path: P2,
        opt: FoldersOptions,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = P1>,
        P1: Into<PathBuf>,
        P2: AsRef<Path>,
    {
        let db_path = db_path.as_ref();
        let allow_symlinks = opt.allow_symlinks;
        let lister = FolderLister::new_with_options(opt);
        let caches = collections_dirs
            .into_iter()
            .map(move |collection_path| {
                let no_cache_opt = collections_options
                    .get(&collection_path)
                    .map(|o| o.no_cache)
                    .unwrap_or(false);
                if no_cache_opt {
                    info!("Collection {:?} is not using cache", collection_path);
                    Ok(CollectionDirect::new(
                        collection_path.clone(),
                        lister.clone(),
                        allow_symlinks,
                    )
                    .into())
                } else {
                    CollectionCache::new(collection_path.clone(), db_path, lister.clone())
                        .map(|mut cache| {
                            cache.run_update_loop();
                            cache
                        })
                        .map(|c| Collection::from(c))
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
            .ok_or_else(|| Error::MissingCollectionCache(collection))
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
    ) -> Result<Vec<AudioFolderShort>> {
        let mut res = self.get_cache(collection)?.search(q);

        res.sort_unstable_by(|a, b| a.compare_as(ordering, b));
        Ok(res)
    }

    pub fn recent(&self, collection: usize, limit: usize) -> Result<Vec<AudioFolderShort>> {
        self.get_cache(collection).map(|cache| cache.recent(limit))
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
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.get_cache(collection)?
            .insert_position(group, path, position, None)
    }

    pub fn insert_position_if_newer<S, P>(
        &self,
        collection: usize,
        group: S,
        path: P,
        position: f32,
        ts: TimeStamp,
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        self.get_cache(collection)?
            .insert_position(group, path, position, Some(ts))
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
    ) -> Result<()>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        spawn_blocking!({
            self.get_cache(collection)?
                .insert_position(group, path, position, None)
        })
        .unwrap_or_else(|e| Err(Error::from(e)))
    }

    pub async fn insert_position_if_newer_async<S, P>(
        self: Arc<Self>,
        collection: usize,
        group: S,
        path: P,
        position: f32,
        ts: TimeStamp,
    ) -> Result<()>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        spawn_blocking!({
            self.get_cache(collection)?
                .insert_position(group, path, position, Some(ts))
        })
        .unwrap_or_else(|e| Err(Error::from(e)))
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

    pub async fn get_all_positions_for_group_async<S>(self: Arc<Self>, group: S) -> Vec<Position>
    where
        S: AsRef<str> + Send + Clone + 'static,
    {
        spawn_blocking!({
            let mut res = vec![];
            for (cn, c) in self.caches.iter().enumerate() {
                let pos = c.get_all_positions_for_group(group.clone(), cn);
                res.extend(pos);
            }
            res.sort_unstable_by(|a, b| b.timestamp.cmp(&a.timestamp));
            res
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
}
