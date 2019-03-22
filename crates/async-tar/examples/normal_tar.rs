extern crate tar;

use std::fs;
use std::env;
use std::path::Path;
//use std::io::prelude::*;

fn main() {
    let dir = env::args().nth(1).expect("Must provide dir as argument");
    let dir_path = Path::new(&dir);
    let parent_dir = dir_path.parent();
    if ! dir_path.is_dir() {
        panic!("Parameter must directory")
    }
    let tar_file = dir_path.file_name().and_then(|n| n.to_str())
        .unwrap_or("current").to_owned()+".tar";
    println!("Archive is {}", tar_file);
    let f = fs::File::create(tar_file).expect("Cannot create file");
    let mut builder = tar::Builder::new(f);
    for entry in fs::read_dir(dir_path).expect("cannot read directory") {
        if let Ok(entry) = entry {
            if let Ok(ft) = entry.file_type() {
                if ft.is_file() {
                    let path = entry.path();
                    println!("Path : {:?}", path);
                    let f = fs::File::open(&path).expect("cannot open file");
                    let meta = f.metadata().unwrap();

                    let archiv_path = match parent_dir {
                        None => path.as_path(),
                        Some(prefix) => path.strip_prefix(prefix).unwrap()
                    };

                    if archiv_path.as_os_str().len()>100 {
                        panic!("Old tar header allows only 100 bytes per name")
                    }

                    let mut header = tar::Header::new_gnu();
                    header.set_path(archiv_path).expect("cannot set path in header");
                    header.set_size(meta.len());
                    header.set_cksum();
                    

                    builder.append(&header, f).unwrap();
                }
            }
        }
    }

    builder.finish().expect("cannot finish archive");

    
}