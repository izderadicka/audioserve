#[macro_use]
extern crate log;

use std::{collections::HashMap, convert::TryInto, path::{Path, PathBuf}, sync::Arc, io};
use audio_folder::FolderLister;
use audio_meta::{AudioFolder};
use error::{Error,Result};
use sled::Db;

pub use audio_folder::{list_dir_files_only, parse_chapter_path};
pub use audio_meta::{init_media_lib, TimeSpan, FoldersOrdering, AudioFile, AudioFolderShort};
pub use util::guess_mime_type;


pub mod error;
pub mod audio_folder;
pub mod audio_meta;
pub mod util;

pub struct Collections {
    caches: HashMap<PathBuf, CollectionCache>,
    lister: FolderLister
}

impl Collections {
    pub fn new() -> Self {
        Collections{
            caches: HashMap::new(),
            lister: FolderLister::new()
        }
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



pub struct CollectionCache {
    db:Db
}

impl CollectionCache {
    pub fn new<P1: AsRef<Path>, P2: AsRef<Path>>(path: P1, db_dir: P2) -> Result<CollectionCache>{
        let db_path = CollectionCache::db_path(path, db_dir)?;
        let db = sled::open(db_path)?;
        Ok(CollectionCache {  
            db
        })
    }

    pub fn f() {

    }

    pub fn update_dir<P: Into<PathBuf>>(&self, dir: PathBuf) -> Result<()> {
        Ok(())
    }

    fn db_path<P1: AsRef<Path>, P2: AsRef<Path>>(path: P1, db_dir: P2) -> Result<PathBuf> {

        let p: &Path = path.as_ref();
        let path_hash = ring::digest::digest(&ring::digest::SHA256, p.to_string_lossy().as_bytes());
        let name_prefix = format!("{:x}",u64::from_be_bytes(path_hash.as_ref()[..8].try_into().expect("Invalid size")));
        let name = p.file_name()
            .map(|name| name .to_string_lossy() + "_" + name_prefix.as_ref())
            .ok_or_else(|| Error::InvalidCollectionPath)?;
        Ok(db_dir.as_ref().join(name.as_ref()))
        
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_db_path() {
        let path = Path::new("nejaka/cesta/na/kolekci");
        let collection_path = CollectionCache::db_path(path, "databaze").unwrap();
        let name = collection_path.file_name().unwrap().to_string_lossy();
        let name:Vec<_> = name.split('_').collect();
        assert_eq!("kolekci", name[0]);
        assert_eq!(16, name[1].len());


    }
}
