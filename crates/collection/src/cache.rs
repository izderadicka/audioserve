use crate::{audio_folder::FolderLister, error::{Error, Result}};
use sled::Db;
use std::{
    convert::TryInto,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};
use walkdir::{DirEntry, WalkDir};

pub struct CollectionCache {
    db: Db,
    root_path: PathBuf,
}

impl CollectionCache {
    pub fn new<P1: Into<PathBuf>, P2: AsRef<Path>>(
        path: P1,
        db_dir: P2,
    ) -> Result<CollectionCache> {
        let root_path = path.into();
        let db_path = CollectionCache::db_path(&root_path, db_dir)?;
        let db = sled::open(db_path)?;
        Ok(CollectionCache { db, root_path })
    }

    pub fn f() {}

    pub fn update_dir<P: Into<PathBuf>>(&self, dir: PathBuf) -> Result<()> {
        Ok(())
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
}

struct Updater {
    db: Arc<Db>,
    thread: Option<thread::JoinHandle<()>>,
    lister:FolderLister
}

impl Updater {
    fn new(db: Arc<Db>, lister:FolderLister) -> Self {
        Updater { db, thread: None , lister}
    }
    fn run(&mut self, root_path: PathBuf) {
        let db = self.db.clone();
        let thread = thread::spawn(move || {
            let walker = WalkDir::new(&root_path).follow_links(false).into_iter();
            for entry in walker.filter_entry(|e| is_visible_dir(e)) {
                match entry {
                    Ok(entry) => {
                        let rel_path = entry
                            .path()
                            .strip_prefix(&root_path)
                            .expect("always have root path")
                            .to_str();
                        if let Some(rel_path) = rel_path {
                            debug!("Got directory {:?}", rel_path);
                            db.insert(rel_path, "")
                                .map_err(|e| error!("Cannot insert to db {}", e))
                                .ok();
                        }
                    }
                    Err(e) => error!("Cannot read directory entry: {}", e),
                }
            }
        });
        self.thread = Some(thread);
    }
}

fn is_visible_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && !entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {

    use tempdir::TempDir;

    use super::*;

    #[test]
    fn test_updater() {
        env_logger::try_init().ok();
        let tmp_dir = TempDir::new("AS_CACHE_TEST").expect("Cannot create temp dir");
        let test_data_dir = Path::new("../../test_data");
        let db_path = tmp_dir.path().join("updater_db");
        let db = Arc::new(sled::open(db_path).expect("Cannot create db"));
        let mut updater = Updater::new(db.clone(), FolderLister::new());
        updater.run(test_data_dir.into());
        updater
            .thread
            .expect("thread was not created")
            .join()
            .expect("thread error");

        let entry1 = db.get("").unwrap().unwrap();
        let entry2 = db.get("usak/kulisak").unwrap().unwrap();
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
}
