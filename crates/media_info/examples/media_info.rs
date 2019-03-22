extern crate media_info;

use media_info::MediaFile;
use std::env;
use std::process::exit;

macro_rules! print_meta {
    ($mf: ident $($name:ident)+) => {
        $(
        if let Some($name) = $mf.$name() {
        println!("{}: {}", stringify!($name),$name);
        }
        )+
    };
}

fn pretty_time(mut time: u64) -> String {
    const HOUR: u64 = 3_600_000;
    const MINUTE: u64 = 60_000;
    let hours = time / HOUR;
    time = time - hours * HOUR;
    let mins = time / MINUTE;
    time = time - mins * MINUTE;
    let secs = time as f64 / 1_000.0;

    return format!("{:02}:{:02}:{:02.3}", hours, mins, secs);
}

fn main() {
    media_info::init();
    let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Must provide file path as param");
        exit(1);
    }

    let fname = &args[1];

    let mf = MediaFile::open(fname).expect(&format!("Cannot open file {}", fname));
    println!("file: {}", fname);
    println!("duration: {}", pretty_time(mf.duration()));
    println!("bitrate: {} kbps", mf.bitrate());
    print_meta!(mf title artist album composer genre);
    if let Some(chapters) = mf.chapters() {
        for chap in chapters {
            println!(
                "Chapter {} - {} ({} - {})",
                chap.num,
                chap.title,
                pretty_time(chap.start as u64),
                pretty_time(chap.end as u64)
            );
        }
    }

    //println!("All meta {:?}", mf.all_meta());
}
