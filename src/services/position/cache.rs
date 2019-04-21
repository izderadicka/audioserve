use crate::config::get_config;
use linked_hash_map::LinkedHashMap;
use serde::Serializer;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

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
    pub fn new(sz: usize) -> Self {
        let fname = &get_config().positions_file;
        if let Ok(f) = fs::File::open(fname) {
            if let Ok(mut inner) = serde_json::from_reader::<_, CacheInner>(f) {
                inner.shrink(sz);
                inner.max_size = sz;
                return Cache {
                    inner: Arc::new(RwLock::new(inner)),
                };
            }
        }

        Cache {
            inner: Arc::new(RwLock::new(CacheInner::new(sz))),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let dir = get_config().positions_file.parent();
        if let Some(d) = dir {
            if !d.exists() {
                fs::create_dir_all(&d)?;
            }
        };

        let fname = &get_config().positions_file;
        let f = fs::File::create(fname)?;
        {
            let c = self.inner.read().unwrap();
            serde_json::to_writer::<_, CacheInner>(f, &c)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }
    }

    pub fn insert<S: Into<String>>(&self, file_path: S, position: f32) {
        self.inner.write().unwrap().insert(file_path, position)
    }

    pub fn get<K>(&self, folder: &K) -> Option<Position>
    where
        K: AsRef<str> +?Sized
    {
        self.inner.read().unwrap().get(folder)
    }

    pub fn get_last<G: AsRef<str>>(&self, group: G) -> Option<Position> {
        self.inner.read().unwrap().get_last(group)
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.inner.write().unwrap().clear()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }
}

#[derive(Serialize, Deserialize)]
struct CacheInner {
    table: HashMap<String, LinkedHashMap<String, PositionRecord>>,
    max_size: usize,
}

impl CacheInner {
    fn new(sz: usize) -> Self {
        CacheInner {
            table: HashMap::new(),
            max_size: sz,
        }
    }

    fn insert<S: Into<String>>(&mut self, group_path: S, position: f32) {
        let group_path = group_path.into();
        if let Some((group, file_path)) = split_group(&group_path) {
            let last_slash = file_path.rfind("/");
            let (folder_path, file) = match last_slash {
                Some(idx) => {
                    let (folder, file) = file_path.split_at(idx);
                    (folder.to_owned(), file[1..].to_owned())
                }

                None => ("".to_owned(), file_path.to_owned()),
            };

            let rec = PositionRecord {
                file,
                position,
                timestamp: SystemTime::now(),
            };

            let table = self.table.entry(group.into())
                .or_insert_with(|| {
                LinkedHashMap::new()
            });
            table.insert(folder_path, rec);
            if table.len() > self.max_size {
                table.pop_front();
            }
            
            
        } else {
            error!("Invalid path, ignoring");
        }
    }

    fn get<K>(&self, group_folder: &K) -> Option<Position>
    where
        K: AsRef<str>+?Sized
    {
        split_group(group_folder)
        .and_then(|(group,folder)| {
            self.table
            .get(group)
            .and_then(|table| 
            table.get(folder).map(|p| to_position(folder.as_ref(), p)))
        })
        
    }

    fn get_last<G: AsRef<str>>(&self, group: G) -> Option<Position> {
            self.table
            .get(group.as_ref())
            .and_then(|table| 
            table.back().map(|(folder,p)| to_position(folder.as_ref(), p)))
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
fn split_group<'a, S: AsRef<str> + ?Sized>(group_path: &'a S) -> Option<(&'a str, &'a str)> {
    let parts = Some(group_path.as_ref().splitn(2, '/'));
    return parts.and_then(|mut p| Some((p.next()?, p.next()?)));
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
        let mut c = CacheInner::new(5);
        let p = c.get_last("group");
        assert!(p.is_none());

        c.insert("group/book1/chap1", 1.1);
        c.insert("group/book2/chap2", 2.1);
        c.insert("group/book3/chap3", 3.1);
        c.insert("group/book4/chap4", 4.1);
        c.insert("group/book5/chap5", 5.1);
        c.insert("group/book6/chap6", 6.1);
        c.insert("group/book4/chap7", 7.1);

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
        let mut c = CacheInner::new(10);
        c.insert("group/book1/chap1", 123.456);
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
        c2.insert("other/book10/chap1", 15.1);
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
}
