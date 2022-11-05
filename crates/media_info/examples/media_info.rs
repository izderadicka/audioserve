extern crate media_info;

use std::{fs::File, io::Write, path::PathBuf};

use clap::Parser;
use media_info::MediaFile;

// macro_rules! print_meta {
//     ($mf: ident $($name:ident)+) => {
//         $(
//         if let Some($name) = $mf.$name() {
//         println!("{}: {}", stringify!($name),$name);
//         }
//         )+
//     };
// }

#[derive(Debug, Parser)]
struct Opts {
    #[arg(name = "FILE", help = "audio file")]
    file_name: String,

    #[arg(long, help = "do not display basic info")]
    no_basic: bool,

    #[arg(long, help = "display chapters info")]
    chapters: bool,

    #[arg(long, help = "do not diplay tags")]
    no_tags: bool,

    #[arg(long, help = "do not diplay streams")]
    no_streams: bool,

    #[arg(long, help = "write cover to file")]
    cover_file: Option<PathBuf>,
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
    let opts = Opts::parse();
    media_info::init();

    let mf =
        MediaFile::open(&opts.file_name).expect(&format!("Cannot open file {}", opts.file_name));

    if !opts.no_basic {
        println!("BASIC INFORMATION:");
        println!("file: {}", opts.file_name);
        println!("duration: {}", pretty_time(mf.duration()));
        println!("bitrate: {} kbps", mf.bitrate());
        println!("has cover image: {}", mf.has_cover());
        println!("has description: {}", mf.has_meta("description"));
        println!();
    }

    if !opts.no_tags {
        let meta = mf.all_meta();
        if meta.len() > 0 {
            println!("META TAGS:");
            let mut keys = meta.keys().collect::<Vec<_>>();
            keys.sort();
            for k in keys {
                println!("{}: {}", k, meta[k]);
            }
            println!();
        }
    }

    if !opts.no_streams && mf.streams_count() > 0 {
        println!("STREAMS:");
        for idx in 0..mf.streams_count() {
            let s = mf.stream(idx);
            println!(
                "Stream type {:?}, codec id {}, 4cc {}({}), duration {}, frames {}, bitrate {}, disposition {}, picture size {}",
                s.kind(),
                s.codec_id(),
                s.codec_four_cc(),
                s.codec_four_cc_raw(),
                s.duration(),
                s.frames_count(),
                s.bitrate(),
                s.disposition(),
                s.picture().map(|p| p.len()).unwrap_or(0)
            );
        }
        println!();
    }

    if let Some(path) = opts.cover_file {
        if let Some(pic_data) = mf.cover() {
            let mut f = File::create(&path).expect(&format!("cannot create file {:?}", path));
            f.write_all(&pic_data).expect("error writing data");
        }
    }

    if opts.chapters {
        if let Some(chapters) = mf.chapters() {
            println!("CHAPTERS:");
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
    }

    //println!("All meta {:?}", mf.all_meta());
}
