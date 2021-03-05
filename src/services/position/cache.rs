use crate::config::get_config;
use crate::error::{Error, Result};
use linked_hash_map::LinkedHashMap;
use serde::Serializer;
use std::fs;
use std::io;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use std::{collections::HashMap, time::Duration};
use tokio::sync::RwLock;

#[derive(Clone, Serialize, Deserialize)]
pub struct PositionRecord {
    file: String,
    timestamp: SystemTime,
    position: f32,
}

#[derive(Clone, Serialize, PartialEq)]
pub struct Position {
    file: String,
    folder: String,
    #[serde(serialize_with = "serialize_ts")]
    timestamp: SystemTime,
    position: f32,
}

fn serialize_ts<S: Serializer>(ts: &SystemTime, ser: S) -> Result<S::Ok, S::Error> {
    let dur = ts
        .duration_since(UNIX_EPOCH)
        .map_err(serde::ser::Error::custom)?;
    let num = dur.as_millis();
    ser.serialize_u64(num as u64)
}

#[derive(Clone)]
pub struct Cache {
    inner: Arc<RwLock<CacheInner>>,
}

impl Cache {
    pub fn new(sz: usize, groups: usize) -> Self {
        let fname = &get_config().positions_file;
        match  fs::File::open(fname) {
        Ok(f)=> {
            match serde_json::from_reader::<_, CacheInner>(f) {
            Ok(mut inner) =>  {
                inner.shrink(sz);
                inner.max_size = sz;
                inner.max_groups = groups;
                return Cache {
                    inner: Arc::new(RwLock::new(inner)),
                };
            }
            Err(e) => error!("Cannot read positions file: {}", e)
        }
        }
        Err(e) => {
            match e.kind() {
                io::ErrorKind::NotFound => debug!("Position file is not present, new will be created"),
                _ => error!("Cannot open positions file: {}, will start with empty one (kill with SIGKILL to preserve old one)",e)
            }
        }
    }

        Cache {
            inner: Arc::new(RwLock::new(CacheInner::new(sz, groups))),
        }
    }

    pub async fn save(&self) -> io::Result<()> {
        let dir = get_config().positions_file.parent();
        if let Some(d) = dir {
            if !d.exists() {
                fs::create_dir_all(&d)?;
            }
        };

        let fname = &get_config().positions_file;
        let f = fs::File::create(fname)?;
        {
            let c = self.inner.read().await;
            serde_json::to_writer::<_, CacheInner>(f, &c)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }
    }

    pub async fn insert<S: Into<String>>(&self, file_path: S, position: f32) -> Result<()> {
        self.inner.write().await.insert(file_path, position)
    }

    pub async fn insert_if_newer<S: Into<String>>(
        &self,
        group_path: S,
        position: f32,
        ts: u64,
    ) -> Result<()> {
        self.inner
            .write()
            .await
            .insert_if_newer(group_path, position, ts)
    }

    pub async fn get<K>(&self, folder: &K) -> Option<Position>
    where
        K: AsRef<str> + ?Sized,
    {
        self.inner.read().await.get(folder)
    }

    pub async fn get_last<G: AsRef<str>>(&self, group: G) -> Option<Position> {
        self.inner.read().await.get_last(group)
    }

    #[allow(dead_code)]
    pub async fn clear(&mut self) {
        self.inner.write().await.clear()
    }

    #[allow(dead_code)]
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }
}

#[derive(Serialize, Deserialize)]
struct CacheInner {
    table: HashMap<String, LinkedHashMap<String, PositionRecord>>,
    max_size: usize,
    max_groups: usize,
}

impl CacheInner {
    fn new(sz: usize, groups: usize) -> Self {
        CacheInner {
            table: HashMap::new(),
            max_size: sz,
            max_groups: groups,
        }
    }

    fn _insert<S, F>(&mut self, group_path: S, position: f32, check_rec: F) -> Result<()>
    where
        S: Into<String>,
        F: FnOnce(&CacheInner, &str, PositionRecord) -> Result<PositionRecord>,
    {
        let group_path = group_path.into();
        if let Some((group, file_path)) = split_group(&group_path) {
            let last_slash = file_path.rfind('/');
            let (folder_path, file) = match last_slash {
                Some(idx) => {
                    let (folder, file) = file_path.split_at(idx);
                    (folder.to_owned(), file[1..].to_owned())
                }

                None => ("".to_owned(), file_path.to_owned()),
            };

            if !self.table.contains_key(group) && self.table.len() >= self.max_groups {
                return Err(Error::msg("Positions cache is full, all groups taken"));
            }

            let mut rec = PositionRecord {
                file,
                position,
                timestamp: SystemTime::now(),
            };
            rec = check_rec(&*self, group, rec)?;
            let table = self
                .table
                .entry(group.into())
                .or_insert_with(LinkedHashMap::new);
            table.insert(folder_path, rec);
            if table.len() > self.max_size {
                table.pop_front();
            }
            Ok(())
        } else {
            Err(Error::msg("Invalid path, ignoring"))
        }
    }

    fn insert_if_newer<S: Into<String>>(
        &mut self,
        group_path: S,
        position: f32,
        ts: u64,
    ) -> Result<()> {
        self._insert(group_path, position, |t, group, mut rec| {
            let diff = rec
                .timestamp
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs()
                .saturating_sub(ts);
            rec.timestamp = rec.timestamp - Duration::from_secs(diff);
            match t.get_last(group) {
                None => Ok(rec),
                Some(last) => {
                    if last.timestamp < rec.timestamp {
                        Ok(rec)
                    } else {
                        Err(Error::msg("There is already newer record"))
                    }
                }
            }
        })
    }

    fn insert<S: Into<String>>(&mut self, group_path: S, position: f32) -> Result<()> {
        self._insert(group_path, position, |_, _, r| Ok(r))
    }

    fn get<K>(&self, group_folder: &K) -> Option<Position>
    where
        K: AsRef<str> + ?Sized,
    {
        split_group(group_folder).and_then(|(group, folder)| {
            self.table
                .get(group)
                .and_then(|table| table.get(folder).map(|p| to_position(folder, p)))
        })
    }

    fn get_last<G: AsRef<str>>(&self, group: G) -> Option<Position> {
        self.table.get(group.as_ref()).and_then(|table| {
            table
                .back()
                .map(|(folder, p)| to_position(folder.as_ref(), p))
        })
    }

    fn clear(&mut self) {
        self.table.clear()
    }

    fn len(&self) -> usize {
        let mut l = 0;
        for table in self.table.values() {
            l += table.len()
        }
        l
    }

    fn shrink(&mut self, sz: usize) {
        for table in self.table.values_mut() {
            while table.len() > sz {
                table.pop_front();
            }
        }
    }
}
fn split_group<S: AsRef<str> + ?Sized>(group_path: &S) -> Option<(&str, &str)> {
    let parts = Some(group_path.as_ref().splitn(2, '/'));
    parts.and_then(|mut p| Some((p.next()?, p.next()?)))
}

fn to_position(folder: &str, r: &PositionRecord) -> Position {
    Position {
        folder: folder.to_owned(),
        file: r.file.clone(),
        position: r.position,
        timestamp: r.timestamp,
    }
}

#[cfg(test)]
mod test {

    use super::*;

    fn make_cache() -> CacheInner {
        let mut c = CacheInner::new(5, 5);
        let p = c.get_last("group");
        assert!(p.is_none());

        c.insert("group/book1/chap1", 1.1).unwrap();
        c.insert("group/book2/chap2", 2.1).unwrap();
        c.insert("group/book3/chap3", 3.1).unwrap();
        c.insert("group/book4/chap4", 4.1).unwrap();
        c.insert("group/book5/chap5", 5.1).unwrap();
        c.insert("group/book6/chap6", 6.1).unwrap();
        c.insert("group/book4/chap7", 7.1).unwrap();

        c
    }

    fn check_cache(c: &CacheInner) {
        assert_eq!(5, c.len());
        assert!(c.get_last("other").is_none());
        let p_last = c.get_last("group").unwrap();
        assert_eq!(
            ("book4", "chap7", 7.1),
            (
                p_last.folder.as_ref(),
                p_last.file.as_ref(),
                p_last.position
            )
        );

        let p_last = c.get("group/book2").unwrap();
        assert_eq!(
            ("book2", "chap2", 2.1),
            (
                p_last.folder.as_ref(),
                p_last.file.as_ref(),
                p_last.position
            )
        );
        assert!(p_last.timestamp <= SystemTime::now());

        let p = c.get("group/book1");
        assert!(p.is_none());
    }

    #[test]
    fn test_positions_cache() {
        let mut c = make_cache();
        check_cache(&c);
        c.clear();
    }

    #[test]
    fn position_serialization() {
        let mut c = CacheInner::new(10, 10);
        c.insert("group/book1/chap1", 123.456).unwrap();
        let p = c.get("group/book1").unwrap();
        let s = serde_json::to_string_pretty(&p).unwrap();
        println!("{}", s);
    }

    #[test]
    fn positions_map_serialization() {
        let c = make_cache();
        let serc = serde_json::to_string(&c).unwrap();
        let mut c2: CacheInner = serde_json::from_str(&serc).unwrap();
        check_cache(&c2);
        c2.insert("other/book10/chap1", 15.1).unwrap();
        let pos = c2.get("other/book10").unwrap();
        assert_eq!(15.1, pos.position);
        let pos = c2.get_last("other").unwrap();
        assert_eq!(15.1, pos.position);
    }

    #[test]
    fn test_split_group() {
        let s = "group/0/adams/stopar";
        let res = split_group(s).unwrap();
        assert_eq!("group", res.0);
        assert_eq!("0/adams/stopar", res.1);
    }

    #[test]
    fn test_max_groups() {
        let mut c = CacheInner::new(5, 3);
        c.insert("g1/book/f", 1.0).unwrap();
        c.insert("g2/book/f", 1.0).unwrap();
        c.insert("g3/book/f", 1.0).unwrap();
        assert!(c.insert("g4/book/f", 1.0).is_err());
    }

    #[test]
    fn test_insert_newer() {
        let mut c = CacheInner::new(5, 3);
        c.insert("g1/book/f", 1.0).unwrap();
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let ts_old = ts - 10;
        assert!(c.insert_if_newer("g1/book/g", 2.0, ts_old).is_err());
        c.insert_if_newer("g2/book/f", 3.0, ts_old).unwrap();
        c.insert_if_newer("g2/book/f", 4.0, ts).unwrap();
        let rec = c.get("g2/book").unwrap();
        assert_eq!(rec.file, "f");
        assert_eq!(rec.position, 4.0);
    }
}
