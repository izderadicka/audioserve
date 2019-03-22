extern crate tar;

use std::fs;
use std::env;
use std::path::Path;
use std::io::prelude::*;

fn main() {
    let file = env::args().nth(1).expect("Must provide dir as argument");
    let file_path = Path::new(&file);
    if ! file_path.is_file() {
        panic!("Parameter must file")
    }

    let mut ar = tar::Archive::new(fs::File::open(&file_path).unwrap());
    let entries = ar.entries().unwrap();
    for entry in entries {
        let mut entry = entry.unwrap();
        let p = entry.path().unwrap().into_owned();

        let mut data_from_archive = vec![];
        entry.read_to_end(&mut data_from_archive).unwrap();
        

        println!("File {:?} entry header start {}, file start {}", p, entry.raw_header_position(), entry.raw_file_position());
        println!("File {:?} archive len {}", p, data_from_archive.len())
        

    }
}