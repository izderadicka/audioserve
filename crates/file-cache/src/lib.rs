extern crate data_encoding;
extern crate linked_hash_map;
extern crate rand;
#[macro_use]
extern crate log;
extern crate byteorder;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use data_encoding::BASE64URL_NOPAD;
use linked_hash_map::LinkedHashMap;
use rand::RngCore;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs::{self, Metadata};
use std::io::{self, Read, Write};
use std::ops::Add;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
//use std::time::SystemTime;

pub use self::error::Error;

#[cfg(feature = "asynch")]
pub use asynch::{Cache as AsyncCache, Finisher};

#[cfg(feature = "asynch")]
mod asynch;
mod error;

const PARTIAL: &str = "partial";
const ENTRIES: &str = "entries";
const INDEX_OLD: &str = "index";
const INDEX: &str = "index_v2";
const MAX_KEY_SIZE: usize = 4096;
const FILE_KEY_LEN: usize = 32;

type Result<T> = std::result::Result<T, Error>;
type CacheInnerType = Arc<RwLock<CacheInner>>;

/// On Unix we use latest of mtime or ctime,
/// otherwise it's modified time from platform specific impl.
#[derive(Debug, Clone, Copy)]
pub enum FileModTime {
    General(SystemTime),
    Unix(u64),
}

impl From<Metadata> for FileModTime {
    fn from(meta: Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let mtime = meta.mtime();
            let ctime = meta.ctime();
            let mod_time = match mtime.cmp(&ctime) {
                Ordering::Greater => mtime * 1000 + meta.mtime_nsec() / 1_000_000,
                Ordering::Less => ctime * 1000 + meta.ctime_nsec() / 1_000_000,
                Ordering::Equal => match meta.mtime_nsec().cmp(&meta.ctime_nsec()) {
                    Ordering::Greater => mtime * 1000 + meta.mtime_nsec() / 1_000_000,
                    _ => ctime * 1000 + meta.ctime_nsec() / 1_000_000,
                },
            };

            FileModTime::Unix(mod_time.try_into().unwrap_or(0))
        }

        #[cfg(not(unix))]
        {
            // It's OK as we are not targeting platforms that do not have modified time
            // and worst can happen cache will get stale on these
            FileModTime::General(meta.modified().unwrap_or(SystemTime::UNIX_EPOCH))
        }
    }
}

impl FileModTime {
    pub fn as_millis(&self) -> u64 {
        match self {
            FileModTime::General(t) => {
                let d = t
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_else(|_| Duration::from_millis(0));
                let t = d.as_millis();
                // Should be OK for all reasonable times
                let res = t.try_into();
                if res.is_err() {
                    error!("SystemTime outside of range of u64");
                }
                res.unwrap_or(u64::MAX)
            }
            FileModTime::Unix(millis) => *millis,
        }
    }

    pub fn now() -> Self {
        FileModTime::General(SystemTime::now())
    }
}

impl Add<Duration> for FileModTime {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        match self {
            FileModTime::General(t) => FileModTime::General(t + rhs),
            FileModTime::Unix(t) => FileModTime::Unix(t + rhs.as_millis() as u64),
        }
    }
}

#[derive(Clone)]
pub struct Cache {
    inner: CacheInnerType,
}

impl Cache {
    pub fn new<P: AsRef<Path>>(root: P, max_size: u64, max_files: u64) -> Result<Self> {
        let root = root.as_ref().into();
        CacheInner::new(root, max_size, max_files).map(|cache| Cache {
            inner: Arc::new(RwLock::new(cache)),
        })
    }

    pub fn add<S: AsRef<str>>(&self, key: S, mtime: FileModTime) -> Result<FileGuard> {
        let key: String = key.as_ref().into();
        let mut c = self.inner.write().expect("Cannot lock cache");
        c.add(key.clone(), mtime).map(move |file| FileGuard {
            cache: self.inner.clone(),
            file,
            key,
        })
    }

    pub fn get<S: AsRef<str>>(&self, key: S, mtime: FileModTime) -> Option<Result<fs::File>> {
        let mut cache = self.inner.write().expect("Cannot lock cache");
        cache.get(key, mtime)
    }

    pub fn save_index(&self) -> Result<()> {
        let cache = self.inner.write().expect("Cannot lock cache");
        cache.save_index()
    }

    pub fn len(&self) -> u64 {
        self.inner.read().unwrap().num_files
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn max_size(&self) -> u64 {
        self.inner.read().unwrap().max_size
    }

    pub fn max_files(&self) -> u64 {
        self.inner.read().unwrap().max_files
    }

    /// return tuple (free_files, free_size)
    pub fn free_capacity(&self) -> (u64, u64) {
        let c = self.inner.read().unwrap();
        (c.max_files - c.num_files, c.max_size - c.size)
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        // if dropping last reference to cache save index
        // TODO: reconsider - also FileGuards can hold reference
        if Arc::strong_count(&self.inner) == 1 {
            if let Err(e) = self.save_index() {
                error!("Error saving cache index: {}", e)
            }
        }
    }
}

pub struct FileGuard {
    cache: Arc<RwLock<CacheInner>>,
    file: fs::File,
    key: String,
}

impl io::Write for FileGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn cleanup<S: AsRef<str>>(cache: &Arc<RwLock<CacheInner>>, key: S) {
    let file_name = {
        let mut cache = cache.write().expect("Cannot lock cache");
        let file_key = cache.opened.remove(key.as_ref());
        file_key.map(|k| cache.partial_path(k))
    };

    debug!("Cleanup for file {:?}", file_name);

    if let Some(file_name) = file_name {
        if file_name.exists() {
            if let Err(e) = fs::remove_file(&file_name) {
                error!("Cannot delete file {:?}, error {}", file_name, e)
            }
        }
    }
}

impl Drop for FileGuard {
    fn drop(&mut self) {
        // need to clean up if opened item was not properly finished
        cleanup(&self.cache, &self.key)
    }
}

impl FileGuard {
    pub fn finish(&mut self) -> Result<()> {
        let mut cache = self.cache.write().expect("Cannot lock cache");
        cache.finish(self.key.clone(), &mut self.file)
    }
}

fn gen_cache_key() -> String {
    let mut random = [0; FILE_KEY_LEN];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut random);
    BASE64URL_NOPAD.encode(&random)
}

fn entry_path_helper<P: AsRef<Path>>(root: &Path, file_key: P) -> PathBuf {
    root.join(ENTRIES).join(file_key)
}

#[derive(Debug, Clone)]
struct FileEntry {
    key: String,
    mtime: u64,
}

impl FileEntry {
    fn new(key: String, mtime: FileModTime) -> Self {
        FileEntry {
            key,
            mtime: mtime.as_millis(),
        }
    }
}

impl AsRef<Path> for FileEntry {
    fn as_ref(&self) -> &Path {
        Path::new(&self.key)
    }
}

struct CacheInner {
    files: LinkedHashMap<String, FileEntry>,
    opened: HashMap<String, FileEntry>,
    root: PathBuf,
    max_size: u64,
    max_files: u64,
    size: u64,
    num_files: u64,
}

fn recreate_dir<P: AsRef<Path>>(dir: P) -> io::Result<bool> {
    let dir = dir.as_ref();
    let is_empty = dir
        .read_dir()
        .map(|mut rd| rd.next().is_none())
        .unwrap_or(false);

    if dir.exists() && !is_empty {
        debug!("Recreating {:?}", dir);
        fs::remove_dir_all(dir)?;
        fs::create_dir(dir)?;
        Ok(true)
    } else {
        Ok(is_empty)
    }
}

macro_rules! get_cleanup {
    ($self:ident, $res:ident, $path:ident, $key:ident) => {

        {

        // Code to use if we wanted to update timestamp of file too, but generally should not be necessary
        // let now = filetime::FileTime::from_system_time();
        // if let Err(e) = filetime::set_file_times(&file_name, now, now) {
        //     error!("Cannot set mtime for file {:?} error {}", file_name, e)
        // }

        // cleanup if file was deleted
        if let Some(Err(_)) = $res {
            if let Some(file_name) = $path {
            if ! file_name.exists() {
                error!("File was deleted for key {} ",$key.as_ref());
                $self.remove($key.as_ref()).ok();

            }
            }
        }

        $res

    }


    };
}

impl CacheInner {
    fn new(root: PathBuf, max_size: u64, max_files: u64) -> Result<Self> {
        let created_root = if !root.exists() {
            fs::create_dir(&root)?;
            true
        } else {
            false
        };
        let entries_path = root.join(ENTRIES);
        if !entries_path.exists() {
            fs::create_dir(&entries_path)?
        }
        let partial_path = root.join(PARTIAL);
        //cleanup previous partial caches
        if !recreate_dir(&partial_path)? {
            fs::create_dir(partial_path)?;
        }

        let mut cache = CacheInner {
            files: LinkedHashMap::new(),
            opened: HashMap::new(),
            root,
            max_size,
            max_files,
            size: 0,
            num_files: 0,
        };
        match cache.load_index() {
            Err(e) => {
                error!("Error loading cache index {}", e);
                recreate_dir(&entries_path)?;
            }
            Ok(false) if !created_root => {
                warn!("No cache index found,");
                recreate_dir(&entries_path)?;
            }
            _ => (),
        }
        Ok(cache)
    }

    fn add(&mut self, key: String, mtime: FileModTime) -> Result<fs::File> {
        if key.len() > MAX_KEY_SIZE {
            return Err(Error::InvalidKey);
        }
        if self.opened.contains_key(&key) {
            return Err(Error::KeyOpened(key));
        } else if self.files.contains_key(&key) {
            return Err(Error::KeyAlreadyExists(key));
        }

        let mut new_file_key: String;
        loop {
            new_file_key = gen_cache_key();
            let new_path = self.partial_path(&new_file_key);
            if !new_path.exists() {
                let f = fs::File::create(&new_path)?;
                self.opened.insert(key, FileEntry::new(new_file_key, mtime));
                return Ok(f);
            }
        }
    }

    fn get_entry_path<S: AsRef<str>>(&mut self, key: S, mtime: FileModTime) -> Option<PathBuf> {
        let root = &self.root;
        let mut is_stalled = false;
        let res = self
            .files
            .get_refresh(key.as_ref())
            .and_then(|entry| {
                let mtime = mtime.as_millis();
                if mtime > entry.mtime {
                    //stall entry
                    is_stalled = true;
                    warn!("Stalled entry {} >{}", mtime, entry.mtime);
                    None
                } else {
                    Some(entry)
                }
            })
            .map(|file_key| entry_path_helper(root, file_key));
        if is_stalled {
            self.remove(&key)
                .map_err(|e| error!("Cannot remove key {} from cache: {}", key.as_ref(), e))
                .ok();
        }

        res
    }

    fn get<S: AsRef<str>>(&mut self, key: S, mtime: FileModTime) -> Option<Result<fs::File>> {
        let file_name = self.get_entry_path(&key, mtime);
        let res = file_name
            .as_ref()
            .map(|file_name| fs::File::open(file_name).map_err(|e| e.into()));

        get_cleanup!(self, res, file_name, key)
    }

    #[allow(dead_code)]
    fn get2<S: AsRef<str>>(
        &mut self,
        key: S,
        mtime: FileModTime,
    ) -> Option<Result<(fs::File, PathBuf)>> {
        let file_name = self.get_entry_path(&key, mtime);
        let res = file_name.as_ref().map(|file_name| {
            fs::File::open(file_name)
                .map_err(|e| e.into())
                .map(|f| (f, file_name.clone()))
        });

        get_cleanup!(self, res, file_name, key)
    }

    // This works only on *nix, as one can delete safely opened files, Windows might require bit different approach
    fn remove_last(&mut self) -> Result<()> {
        if let Some((_, file_key)) = self.files.pop_front() {
            let file_path = self.entry_path(file_key);
            let file_size = fs::metadata(&file_path)?.len();
            fs::remove_file(file_path)?;
            self.num_files -= 1;
            self.size -= file_size;
        }
        Ok(())
    }

    fn remove<S: AsRef<str>>(&mut self, key: S) -> Result<()> {
        if let Some(file_key) = self.files.remove(key.as_ref()) {
            let file_path = self.entry_path(file_key);
            self.num_files -= 1;
            match fs::metadata(&file_path) {
                Ok(meta) => {
                    let file_size = meta.len();
                    self.size -= file_size;
                }
                Err(e) => {
                    error!(
                        "Cannot get meta for file {:?} due to error {}",
                        file_path, e
                    );
                    // this means that index is out of sync with fs - recalculate cache size
                    let mut new_size = 0;
                    for (_, file_key) in &self.files {
                        let fname = entry_path_helper(&self.root, file_key);
                        if let Ok(meta) = fs::metadata(fname) {
                            new_size += meta.len()
                        }
                    }
                    self.size = new_size;
                }
            }

            fs::remove_file(file_path)?;
        }
        Ok(())
    }

    fn finish(&mut self, key: String, file: &mut fs::File) -> Result<()> {
        let file_key = match self.opened.remove(&key) {
            Some(key) => key,
            None => return Err(Error::InvalidCacheState("Missing opened key".into())),
        };
        file.flush()?;
        let new_file_size = file.metadata()?.len();
        if new_file_size > self.max_size {
            return Err(Error::FileTooBig);
        }
        let old_path = self.partial_path(file_key.clone());
        while self.size + new_file_size > self.max_size || self.num_files + 1 > self.max_files {
            self.remove_last()?
        }
        let new_path = self.entry_path(&file_key);
        fs::rename(old_path, &new_path)?;
        self.files.insert(key, file_key);
        self.num_files += 1;
        self.size += new_path.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(())
    }

    fn entry_path<P: AsRef<Path>>(&self, file_key: P) -> PathBuf {
        entry_path_helper(&self.root, file_key)
    }

    fn partial_path<P: AsRef<Path>>(&self, file_key: P) -> PathBuf {
        self.root.join(PARTIAL).join(file_key)
    }

    fn save_index(&self) -> Result<()> {
        let tmp_index = self.root.join(String::from(INDEX) + ".tmp");
        {
            let mut f = fs::File::create(&tmp_index)?;
            for (key, value) in self.files.iter() {
                f.write_u16::<BigEndian>(key.len() as u16)?;
                f.write_all(key.as_bytes())?;
                f.write_u64::<BigEndian>(value.mtime)?;
                f.write_u16::<BigEndian>(value.key.len() as u16)?;
                f.write_all(value.key.as_bytes())?;
            }
        }
        fs::rename(tmp_index, self.root.join(INDEX))?;

        Ok(())
    }

    fn load_index(&mut self) -> Result<bool> {
        let tmp_index = self.root.join(String::from(INDEX) + ".tmp");
        if tmp_index.exists() {
            warn!("Found unfinished index, previous save failed, cache might be empty now");
            fs::remove_file(&tmp_index)
                .map_err(|e| error!("Cannot delete tmp index {:?}:{}", tmp_index, e))
                .ok();
        }
        let old_index_path = self.root.join(INDEX_OLD);
        if old_index_path.exists() {
            warn!("Found previous version of cache, will clean and start empty");
            fs::remove_file(&old_index_path)
                .map_err(|e| error!("Cannot delete old index {:?}: {}", old_index_path, e))
                .ok();
            return Ok(false);
        }
        let index_path = self.root.join(INDEX);

        if index_path.exists() {
            let mut index = LinkedHashMap::<String, FileEntry>::new();
            let mut f = fs::File::open(index_path)?;

            loop {
                let key_len = match f.read_u16::<BigEndian>() {
                    Ok(l) => l as usize,
                    Err(e) => match e.kind() {
                        io::ErrorKind::UnexpectedEof => break,
                        _ => return Err(e.into()),
                    },
                };

                if key_len > MAX_KEY_SIZE {
                    return Err(Error::InvalidIndex);
                }

                let mut buf = [0_u8; MAX_KEY_SIZE];
                f.read_exact(&mut buf[..key_len])?;
                let key = String::from_utf8(Vec::from(&buf[..key_len]))
                    .map_err(|_| Error::InvalidIndex)?;
                let mtime = f.read_u64::<BigEndian>()?;
                let value_len = f.read_u16::<BigEndian>()? as usize;
                if value_len > 2 * FILE_KEY_LEN {
                    return Err(Error::InvalidIndex);
                }
                f.read_exact(&mut buf[..value_len])?;
                let value = String::from_utf8(Vec::from(&buf[..value_len]))
                    .map_err(|_| Error::InvalidIndex)?;
                let file_path = self.entry_path(&value);
                if file_path.exists() {
                    let file_size = fs::metadata(&file_path)?.len();
                    // cleanup files over limit
                    if self.num_files + 1 > self.max_files || self.size + file_size > self.max_size
                    {
                        fs::remove_file(&file_path)?;
                        warn!("Removing file above limit {:?}", file_path);
                    } else {
                        index.insert(key, FileEntry { key: value, mtime });
                        self.num_files += 1;
                        self.size += file_size;
                    }
                } else {
                    warn!("Nonexisting file {:?} in index", file_path);
                }
            }

            //cleanup files not in index
            {
                let file_keys_set = index.values().map(|e| &e.key).collect::<HashSet<&String>>();
                let base_dir = self.root.join(ENTRIES);
                if let Ok(dir_list) = fs::read_dir(base_dir) {
                    for dir_entry in dir_list.flatten() {
                        if dir_entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                            if let Ok(file_name) = dir_entry.file_name().into_string() {
                                if !file_keys_set.contains(&file_name) {
                                    fs::remove_file(dir_entry.path()).ok();
                                    warn!("Removing file not in index {:?}", dir_entry.path());
                                }
                            }
                        }
                    }
                }
            }

            self.files = index;
            Ok(true)
        } else {
            debug!("No index file");
            Ok(false)
        }
    }
}

#[cfg(test)]
extern crate env_logger;
#[cfg(test)]
extern crate tempfile;
#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_filemodtime() {
        let now = SystemTime::now();
        let temp_file = tempfile::tempfile().unwrap();
        let meta = temp_file.metadata().unwrap();
        let t: FileModTime = meta.into();
        let mod_time = t.as_millis() as i64;
        let start = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let diff = (start - mod_time).abs();
        assert!(diff < 1000)
    }

    #[test]
    fn basic_stalled() {
        env_logger::try_init().ok();
        const MY_KEY: &str = "muj_test_1";
        let temp_dir = tempdir().unwrap();

        let t = FileModTime::now();

        let msg = "Hello there";
        {
            let c = Cache::new(temp_dir.path(), 10000, 10).unwrap();
            {
                let mut f = c.add(MY_KEY, t).unwrap();

                f.write(msg.as_bytes()).unwrap();
                f.finish().unwrap();
            }
            let f = c.get(MY_KEY, t + Duration::from_secs(1));
            assert!(f.is_none());
        }
    }

    #[test]
    fn basic_test() {
        env_logger::try_init().ok();
        const MY_KEY: &str = "muj_test_1";
        let temp_dir = tempdir().unwrap();

        let t = FileModTime::now();

        let msg = "Hello there";
        {
            let c = Cache::new(temp_dir.path(), 10000, 10).unwrap();
            {
                let mut f = c.add(MY_KEY, t).unwrap();

                f.write(msg.as_bytes()).unwrap();
                f.finish().unwrap();
            }
            let mut f = c.get(MY_KEY, t).unwrap().unwrap();

            let mut msg2 = String::new();
            f.read_to_string(&mut msg2).unwrap();
            assert_eq!(msg, msg2);
            let num_files = c.inner.read().unwrap().num_files;
            assert_eq!(1, num_files);
        }

        {
            let c = Cache::new(temp_dir.path(), 10000, 10).unwrap();
            let mut f = c.get(MY_KEY, t).unwrap().unwrap();

            let mut msg2 = String::new();
            f.read_to_string(&mut msg2).unwrap();
            assert_eq!(msg, msg2);
            let num_files = c.inner.read().unwrap().num_files;
            assert_eq!(1, num_files)
        }
    }

    #[test]
    fn test_cleanup_if_deleted() {
        env_logger::try_init().ok();
        const MY_KEY: &str = "muj_test_1";
        let temp_dir = tempdir().unwrap();
        let t = FileModTime::now();

        let msg = "Hello there";
        {
            let c = Cache::new(temp_dir.path(), 10000, 10).unwrap();
            {
                let mut f = c.add(MY_KEY, t).unwrap();
                f.write(msg.as_bytes()).unwrap();
                f.finish().unwrap();
                let mut f = c.add("second", t).unwrap();
                f.write("0123456789".as_bytes()).unwrap();
                f.finish().unwrap();
            }
            let (_f, fname) = {
                let mut cache = c.inner.write().unwrap();
                cache.get2(MY_KEY, t).unwrap().unwrap()
            };
            fs::remove_file(fname).unwrap();

            if let Some(Err(_)) = c.get(MY_KEY, t) {
                let num_files = c.inner.read().unwrap().num_files;
                assert_eq!(1, num_files);
                let size = c.inner.read().unwrap().size;
                assert_eq!(10, size);
            } else {
                panic!("get should return error, if file was deleted");
            }
        }
    }

    #[test]
    fn test_many_concurrently() {
        use std::thread;
        env_logger::try_init().ok();
        let tmp_folder = tempdir().unwrap();
        let t = FileModTime::now();

        fn test_cache(c: &Cache, t: FileModTime) {
            {
                let cache = c.inner.read().unwrap();
                assert_eq!(5, cache.files.len());
            }
            let mut count = 0;
            for i in 0..10 {
                match c.get(&format!("Key {}", i), t) {
                    None => (),
                    Some(res) => {
                        let mut f = res.unwrap();
                        let mut s = String::new();
                        f.read_to_string(&mut s).unwrap();
                        assert_eq!(format!("Cached content {}", i), s);
                        count += 1;
                    }
                }
            }

            assert_eq!(5, count);
        }

        {
            let mut threads = Vec::new();
            let c = Cache::new(tmp_folder.path(), 10_000, 5).unwrap();
            for i in 0..10 {
                let c = c.clone();
                threads.push(thread::spawn(move || {
                    let mut f = c.add(format!("Key {}", i), t).unwrap();
                    let msg = format!("Cached content {}", i);
                    f.write_all(msg.as_bytes()).unwrap();
                    f.finish().unwrap();
                }));
            }

            for t in threads {
                t.join().unwrap();
            }

            test_cache(&c, t);
        }

        {
            let c = Cache::new(tmp_folder.path(), 10_000, 5).unwrap();
            test_cache(&c, t);
        }
    }

    #[test]
    fn test_size() {
        use rand::Rng;
        use std::thread;

        env_logger::try_init().ok();
        let tmp_folder = tempdir().unwrap();

        let mut data = [0_u8; 1024];
        let mut rng = rand::rng();
        let t = FileModTime::now();
        rng.fill_bytes(&mut data);

        fn test_cache(c: &Cache, data: &[u8], t: FileModTime) {
            {
                let cache = c.inner.read().unwrap();
                assert_eq!(5, cache.files.len());
            }
            let mut count = 0;
            for i in 0..10 {
                match c.get(&format!("Key {}", i), t) {
                    None => (),
                    Some(res) => {
                        let mut f = res.unwrap();
                        let mut s = Vec::new();
                        f.read_to_end(&mut s).unwrap();
                        assert_eq!(data, &s[..]);
                        count += 1;
                    }
                }
            }

            assert_eq!(5, count);
        }

        {
            let mut threads = Vec::new();
            let c = Cache::new(tmp_folder.path(), 6_000, 1000).unwrap();
            for i in 0..10 {
                let c = c.clone();
                threads.push(thread::spawn(move || {
                    let mut f = c.add(format!("Key {}", i), t).unwrap();
                    let mut rng = rand::rng();
                    for j in 0..8 {
                        f.write_all(&data[128 * j..128 * (j + 1)]).unwrap();
                        thread::sleep(std::time::Duration::from_millis(
                            rng.random_range(Range { start: 1, end: 100 }),
                        ))
                    }
                    f.finish().unwrap();
                }));
            }

            for t in threads {
                t.join().unwrap();
            }

            test_cache(&c, &data, t);
        }
    }

    #[test]
    fn test_cleanup() {
        env_logger::try_init().ok();
        let tmp_folder = tempdir().unwrap();
        let c = Cache::new(tmp_folder.path(), 10_000, 50).unwrap();
        let p = c.inner.read().unwrap().partial_path("test");
        let p = p.parent().unwrap();
        let list_path = || {
            use std::fs;
            fs::read_dir(p).unwrap().count()
        };

        {
            let _f = c.add("usak", FileModTime::now());
            assert_eq!(1, list_path());
        }

        assert_eq!(0, list_path())
    }
}
