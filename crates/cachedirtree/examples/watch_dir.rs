extern crate cachedirtree;
extern crate env_logger;
#[macro_use]
extern crate log;

use cachedirtree::*;
use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

fn main() -> io::Result<()> {
    env_logger::init();
    let path = env::args().nth(1).unwrap();
    let opts = OptionsBuilder::default()
        .watch_changes(true)
//        .include_files(false)
        .build()
        .unwrap();
    let c = DirCache::new_with_options(&path, opts);
    let server = TcpListener::bind("127.0.0.1:54321")?;
    info!("Listening on port 54321");
    c.wait_ready();
    info!("Directory cached");
    for stream in server.incoming() {
        let mut stream = stream?;
        let c = c.clone();
        thread::spawn(move || {
            let mut query = String::new();
            {
                let mut r = BufReader::new(&stream);
                r.read_line(&mut query).unwrap();
            }
            let res = c.search(query).unwrap();
            if res.is_empty() {
                stream.write(b"Nothing found!\n").unwrap();
            } else {
                stream.write(b"Search results:\n").unwrap();
                for p in res {
                    stream.write(p.to_str().unwrap().as_bytes()).unwrap();
                    stream.write(b"\n").unwrap();
                }
            }
        });
    }

    Ok(())
}
