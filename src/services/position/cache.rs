use linked_hash_map::LinkedHashMap;
use std::time::SystemTime;
use std::borrow::Borrow;
use std::hash::Hash;


pub struct PositionRecord {
    file: String,
    timestamp: SystemTime,
    position: f32


}

pub struct Position {
    file: String,
    folder: String,
    timestamp: SystemTime,
    position: f32
}



pub struct Cache {
    table: LinkedHashMap<String, PositionRecord>,
    max_size: usize
}

impl Cache {
    pub fn new(sz:usize) -> Self {
        Cache {
            table: LinkedHashMap::new(),
            max_size:sz
        }
    }

    pub fn insert<S: Into<String>>(&mut self, file_path: S, position: f32) {
        let file_path = file_path.into();
        let last_slash = file_path.rfind("/");
        let (folder_path, file) = match last_slash {
            Some(idx) => {
                let (folder,file) = file_path.split_at(idx);
                (folder.to_owned(), file[1..].to_owned())
            }

            None => ("".to_owned(), file_path)
        };

        let rec = PositionRecord {
            file,
            position,
            timestamp: SystemTime::now()
            
        };

        self.table.insert(folder_path, rec);

        if self.table.len() > self.max_size {
            self.table.pop_front();
        }


    }

    pub fn get<K>(&self, folder:&K) -> Option<Position>
    where String: Borrow<K>,
        K: AsRef<str>,
        K:Hash+Eq+?Sized
     {
         self.table.get(folder)
         .map(|p| to_position(folder.as_ref(), p))
        
    }

    pub fn get_last(&self) -> Option<Position> {
        self.table.back()
        .map(|(folder, r)| to_position(folder, r))
    }

    pub fn clear(&mut  self) {
        self.table.clear()
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }
}

fn to_position(folder: &str, r: &PositionRecord) -> Position {
    Position {
        folder: folder.to_owned(),
        file: r.file.clone(),
        position: r.position,
        timestamp: r.timestamp
    }
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_positions_cache() {

        let mut c = Cache::new(5);
        let p = c.get_last();
        assert!(p.is_none());

        c.insert("book1/chap1", 1.1);
        c.insert("book2/chap2", 2.1);
        c.insert("book3/chap3", 3.1);
        c.insert("book4/chap4", 4.1);
        c.insert("book5/chap5", 5.1);
        c.insert("book6/chap6", 6.1);
        c.insert("book4/chap7", 7.1);

        assert_eq!(5,c.len());

        let p_last = c.get_last().unwrap();

        assert_eq!(("book4", "chap7", 7.1), (p_last.folder.as_ref(), p_last.file.as_ref(), p_last.position));


        let p_last = c.get("book2").unwrap();

        assert_eq!(("book2", "chap2", 2.1), (p_last.folder.as_ref(), p_last.file.as_ref(), p_last.position));

        assert!(p_last.timestamp<= SystemTime::now());

        let p = c.get("book1");

        assert!(p.is_none());

        c.clear();


    }

}
