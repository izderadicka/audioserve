#[macro_use]
extern crate log;

use audio_folder::{FolderLister, FoldersOptions};
use audio_meta::AudioFolder;
use cache::CollectionCache;
use error::Result;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

pub use audio_folder::{list_dir_files_only, parse_chapter_path};
pub use audio_meta::{init_media_lib, AudioFile, AudioFolderShort, FoldersOrdering, TimeSpan};
pub use util::guess_mime_type;

pub mod audio_folder;
pub mod audio_meta;
pub mod error;
pub mod util;
mod cache;

pub struct Collections {
    caches: HashMap<PathBuf, CollectionCache>,
    lister: FolderLister,
}

impl Collections {
    pub fn new() -> Self {
        Collections {
            caches: HashMap::new(),
            lister: FolderLister::new(),
        }
    }

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
        for d in collections_dirs.into_iter() {
            let collection_path: PathBuf = d.into();
            let cache = CollectionCache::new(collection_path.clone(), db_path)?;
            caches.insert(collection_path, cache);
        }
        Ok(Collections {
            caches,
            lister: FolderLister::new_with_options(opt),
        })
    }
}

impl Collections {
    pub fn list_dir<P: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        base_dir: P,
        dir_path: P2,
        ordering: FoldersOrdering,
    ) -> std::result::Result<AudioFolder, io::Error> {
        self.lister.list_dir(base_dir, dir_path, ordering)
    }
}

