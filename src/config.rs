use clap::{App, Arg};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::env;
use std::io;

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
    pub base_dir: PathBuf
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
        return Err("Too low - should be above 10".into())
    }
    if max_sending_threads > 10000 {
        return Err("Too much - should be below 10000".into())
    }


    Ok(Config{
        base_dir,
        local_addr,
        max_sending_threads
    })

}