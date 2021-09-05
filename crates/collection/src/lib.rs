#[macro_use]
extern crate log;

use std::{path::{Path, PathBuf}, sync::Arc};
use error::{Error,Result};
use sled::Db;

pub mod error;
pub mod audio_folder;
pub mod audio_meta;
pub mod util;

#[derive(Clone)]
pub struct Collection {
    db:Arc<Db>
}

impl Collection {
    pub fn new<P1: AsRef<Path>, P2: AsRef<Path>>(path: P1, db_dir: P2) -> Result<Collection>{
        let db_path = Collection::db_path(path, db_dir)?;
        let db = sled::open(db_path)?;
        Ok(Collection {  
            db: Arc::new(db)
        })
    }

    pub fn f() {

    }

    pub fn update_dir<P: Into<PathBuf>>(&self, dir: PathBuf) -> Result<()> {
        Ok(())
    }

    fn db_path<P1: AsRef<Path>, P2: AsRef<Path>>(path: P1, db_dir: P2) -> Result<PathBuf> {

        let p: &Path = path.as_ref();
        let name = p.file_name()
            .map(|name| name.to_string_lossy())
            .ok_or_else(|| Error::InvalidCollectionPath)?;
        Ok(db_dir.as_ref().join(name.as_ref()))
        
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
