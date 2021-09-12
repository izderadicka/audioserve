#[macro_use]
extern crate log;

use audio_folder::{FolderLister, FoldersOptions};
use audio_meta::AudioFolder;
use cache::CollectionCache;
use error::{Error, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub use audio_folder::{list_dir_files_only, parse_chapter_path};
pub use audio_meta::{init_media_lib, AudioFile, AudioFolderShort, FoldersOrdering, TimeSpan};
pub use util::guess_mime_type;

pub mod audio_folder;
pub mod audio_meta;
mod cache;
pub mod error;
pub mod util;

pub struct Collections {
    caches: HashMap<PathBuf, CollectionCache>,
}

impl Collections {
    pub fn new_with_detail<I, P1, P2>(
        collections_dirs: Vec<PathBuf>,
        db_path: P2,
        opt: FoldersOptions,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = P1>,
        P1: Into<PathBuf>,
        P2: AsRef<Path>,
    {
        let mut caches = HashMap::new();
        let db_path = db_path.as_ref();
        let lister = FolderLister::new_with_options(opt);
        for d in collections_dirs.into_iter() {
            let collection_path: PathBuf = d.into();
            let mut cache = CollectionCache::new(collection_path.clone(), db_path, lister.clone())?;
            cache.run_update_loop(collection_path.clone());
            caches.insert(collection_path, cache);
        }
        Ok(Collections { caches })
    }
}

impl Collections {
    pub fn list_dir<P: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        base_dir: P,
        dir_path: P2,
        ordering: FoldersOrdering,
    ) -> Result<AudioFolder> {
        self.caches
            .get(base_dir.as_ref())
            .ok_or_else(|| {
                Error::MissingCollectionCache(base_dir.as_ref().to_string_lossy().into())
            })?
            .list_dir(base_dir, dir_path, ordering)
    }
}
