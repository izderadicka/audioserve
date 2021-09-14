#[macro_use]
extern crate log;

use audio_folder::{FolderLister, FoldersOptions};
use audio_meta::AudioFolder;
use cache::CollectionCache;
use error::{Error, Result};
use std::path::{Path, PathBuf};

pub use audio_folder::{list_dir_files_only, parse_chapter_path};
pub use audio_meta::{init_media_lib, AudioFile, AudioFolderShort, FoldersOrdering, TimeSpan};
pub use util::guess_mime_type;

pub mod audio_folder;
pub mod audio_meta;
mod cache;
pub mod error;
pub mod util;

pub struct Collections {
    caches: Vec<CollectionCache>,
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
        let db_path = db_path.as_ref();
        let lister = FolderLister::new_with_options(opt);
        let caches = collections_dirs
            .into_iter()
            .map(|collection_path| {
                CollectionCache::new(collection_path.clone(), db_path, lister.clone()).map(
                    |mut cache| {
                        cache.run_update_loop(collection_path.clone());
                        cache
                    },
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Collections { caches })
    }
}

impl Collections {
    pub fn list_dir<P: AsRef<Path>>(
        &self,
        collection: usize,
        dir_path: P,
        ordering: FoldersOrdering,
    ) -> Result<AudioFolder> {
        self.caches
            .get(collection)
            .ok_or_else(|| Error::MissingCollectionCache(collection))?
            .list_dir(dir_path, ordering)
    }

    pub fn flush(&self) -> Result<()> {
        let mut result = vec![];
        for c in &self.caches {
            result.push(c.flush())
        }
        result.into_iter().find(|r| r.is_err()).unwrap_or(Ok(()))
    }
}
