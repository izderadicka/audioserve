use crate::{
    audio_folder::FolderLister,
    audio_meta::AudioFolder,
    error::{Error, Result},
    FoldersOrdering,
};
use sled::Db;
use std::{convert::TryInto, path::{Path, PathBuf}, sync::Arc, thread, time::SystemTime};
use walkdir::{DirEntry, WalkDir};

#[derive(Clone)]
struct CacheInner {
    db: Arc<Db>,
    lister: FolderLister,
}

impl CacheInner {
    fn get<P: AsRef<Path>>(&self, dir: P) -> Option<AudioFolder> {
        dir.as_ref()
            .to_str()
            .and_then(|p| {
                self.db
                    .get(p)
                    .map_err(|e| error!("Cannot get record for db: {}", e))
                    .ok()
                    .flatten()
            })
            .and_then(|data| {
                bincode::deserialize(&data)
                    .map_err(|e| error!("Error deserializing data from db {}", e))
                    .ok()
            })
    }

    fn get_if_actual<P: AsRef<Path>>(&self, dir: P, ts: Option<SystemTime>)  -> Option<AudioFolder> {
        let af = self.get(dir);
        af.as_ref()
                                .and_then(|af| af.last_modification)
                                .and_then(|cached_time| {
                                    ts.map(|actual_time| cached_time >= actual_time)
                                })
                                .and_then(|actual| if actual {af} else {None})
    }
}

pub struct CollectionCache {
    thread: Option<thread::JoinHandle<()>>,
    inner: CacheInner,
}

impl CollectionCache {
    pub fn new<P1: AsRef<Path>, P2: AsRef<Path>>(
        path: P1,
        db_dir: P2,
        lister: FolderLister,
    ) -> Result<CollectionCache> {
        let root_path = path.as_ref();
        let db_path = CollectionCache::db_path(&root_path, db_dir)?;
        let db = sled::open(db_path)?;
        Ok(CollectionCache {
            inner: CacheInner {
                db: Arc::new(db),
                lister,
            },
            thread: None,
        })
    }

    pub fn list_dir<P: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        base_dir: P,
        dir_path: P2,
        ordering: FoldersOrdering,
    ) -> Result<AudioFolder> {
        let full_path =base_dir.as_ref().join(&dir_path);
        let ts = full_path.metadata().ok().and_then(|m| m.modified().ok());
        self.inner.get_if_actual(&dir_path, ts)
        .ok_or_else(|| {
            debug!("Fetching folder {:?} from file file system", dir_path.as_ref());
        self.inner
            .lister
            .list_dir(base_dir, dir_path, ordering)
            .map_err(Error::from)
        })
        .or_else(|r| r)
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

    pub fn run_update_loop(&mut self, root_path: PathBuf) {
        let inner = self.inner.clone();
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
                        let mod_ts = entry.metadata().ok().and_then(|m| m.modified().ok());
                        if let Some(rel_path) = rel_path {
                            
                            if inner.get_if_actual(rel_path, mod_ts).is_none() {
                                match inner
                                    .lister
                                    .list_dir(&root_path, rel_path, FoldersOrdering::Alphabetical)
                                    .map_err(Error::from)
                                    .and_then(|af| bincode::serialize(&af).map_err(Error::from))
                                {
                                    Ok(data) => {
                                        inner
                                            .db
                                            .insert(rel_path, data)
                                            .map_err(|e| error!("Cannot insert to db {}", e))
                                            .map(|p| debug!("Path {:?} was cached", entry.path()))
                                            .ok();
                                    }
                                    Err(e) => error!(
                                        "Cannot listing audio folder {:?}, error {}",
                                        entry.path(),
                                        e
                                    ),
                                }
                            } else {
                                debug!("For path {:?} using cached data", entry.path())
                            }
                        } else {
                            error!("Path in collection is not UTF8 {:?}", entry.path());
                        }
                    }
                    Err(e) => error!("Cannot read directory entry: {}", e),
                }
            }
        });
        self.thread = Some(thread);
    }

    pub fn get<P: AsRef<Path>>(&self, dir: P) -> Option<AudioFolder> {
        self.inner.get(dir)
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
        let mut col = CollectionCache::new(test_data_dir, db_path, FolderLister::new())
            .expect("Cannot create CollectionCache");
        col.run_update_loop(test_data_dir.into());
        col.thread
            .take()
            .expect("thread was not created")
            .join()
            .expect("thread error");

        let entry1 = col.get("").unwrap();
        let entry2 = col.get("usak/kulisak").unwrap();
        assert_eq!(2, entry1.files.len());
        assert_eq!(2, entry1.subfolders.len());
        assert_eq!(0, entry2.files.len())
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
