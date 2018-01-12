use clap::{App, Arg};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::env;
use std::io;
use super::services::transcode::Quality;
use num_cpus;

quick_error! { 
#[derive(Debug)]
pub enum Error {
    
    InvalidNumber(err: ::std::num::ParseIntError) {
        from()
    }

    InvalidAddress(err: ::std::net::AddrParseError) {
        from()
    }

    InvalidBaseDirectory(err: ::std::io::Error) {
        from()
    }
    
    InvalidLimitValue(err: &'static str) {
        from()
    }
}
}

#[derive(Debug)]
pub struct Config{
    pub local_addr: SocketAddr,
    pub max_sending_threads: usize,
    pub base_dir: PathBuf,
    pub shared_secret: String,
    pub transcoding: Option<Quality>,
    pub max_transcodings: usize

}
type Parser<'a> = App<'a, 'a>;

fn create_parser<'a>() -> Parser<'a> {
    App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .arg(Arg::with_name("debug")
            .short("d")
            .long("debug")
            .help("Enable debug logging (detailed logging config can be done via RUST_LOG env. variable)")
        )
        .arg(Arg::with_name("local_addr")
            .short("l")
            .long("listen")
            .help("Address and port server is listening on as address:port")
            .takes_value(true)
            .default_value("0.0.0.0:3000")
        )
        .arg(Arg::with_name("max-threads")
            .short("m")
            .long("max-threads")
            .takes_value(true)
            .help("Maximum number of threads for requests processing")
            .default_value("100")
        )
        .arg(Arg::with_name("base_dir")
            .value_name("BASE_DIR")
            .required(true)
            .takes_value(true)
            .help("Root directory for audio books")

        )
        .arg(Arg::with_name("shared-secret")
            .short("s")
            .long("shared-secret")
            .takes_value(true)
            .required(true)
            .help("Shared secret for client authentication")
        )
        .arg(Arg::with_name("transcode")
            .short("t")
            .long("transcode")
            .takes_value(true)
            .possible_values(&["low", "medium", "high"])
            .help("Use transcoding to safe bandwidth (or serve incompatible audio files)")
        )
        .arg(Arg::with_name("max-transcodings")
            .short("x")
            .long("max-transcodings")
            .takes_value(true)
            .help("Maximum number of concurrent transcodings (default is number of cores")
        )
}

pub fn parse_args() -> Result<Config, Error>{
    let p = create_parser();
    let args = p.get_matches();

    if args.is_present("debug") {
        let name = "RUST_LOG";
        if env::var_os(name).is_none() {
            env::set_var(name, "debug");
        }
    }

    let base_dir: PathBuf = args.value_of("base_dir").unwrap().into();
    if ! base_dir.is_dir() {
        return Err(Error::InvalidBaseDirectory(io::Error::from(io::ErrorKind::NotFound)))
    }
    let local_addr = args.value_of("local_addr").unwrap().parse()?;
    let max_sending_threads = args.value_of("max-threads").unwrap().parse()?;
    if max_sending_threads < 10 {
        return Err("Too few threads - should be above 10".into())
    }
    if max_sending_threads > 10000 {
        return Err("Too much threads - should be below 10000".into())
    }
    let shared_secret = args.value_of("shared-secret").unwrap().into();

    let transcoding = args.value_of("transcode").map(|t| match t {
        "low" => Quality::Low,
        "medium" => Quality::Medium,
        "high" => Quality::High,
        _ => unreachable!("Wrong transcoding")
    });

    let max_transcodings = match args.value_of("max-transcodings") {
        Some(s) => {
            s.parse()?
        },
        None => {
            num_cpus::get()
        }
    };
    if max_transcodings < 1 {
        return Err("At least one concurrent trancoding must be available".into())
    } else if max_transcodings > max_sending_threads {
        return Err("Number of concurrent transcodings cannot be higher then number of threads".into())
    }

    Ok(Config{
        base_dir,
        local_addr,
        max_sending_threads,
        shared_secret, 
        transcoding,
        max_transcodings

    })

}