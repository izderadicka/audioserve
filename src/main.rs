extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;
extern crate mime;
extern crate mime_guess;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate clap;
#[macro_use]
extern crate quick_error;
extern crate url;
extern crate percent_encoding;
extern crate taglib;
extern crate num_cpus;
extern crate ring;
extern crate data_encoding;


use hyper::server::{Http as HttpServer};
use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize};
use std::sync::Arc;
use services::{Factory, TranscodingDetails};
use services::auth::SharedSecretAuthenticator;
use services::search::Search;
use services::transcode::Transcoder;
use config::{parse_args, Config};

mod services;
mod config;


fn start_server(config: Config) -> Result<(), hyper::Error> {
    
    let factory = Factory {
        sending_threads: Arc::new(AtomicUsize::new(0)),
        max_threads: config.max_sending_threads,
        base_dir: config.base_dir,
        authenticator: Arc::new(Box::new(SharedSecretAuthenticator::new(
            config.shared_secret,
            "how".into(),
            24
        ))),
        search:Search::FoldersSearch,
        transcoding: TranscodingDetails {
        transcoder: config.transcoding.map(|q| Transcoder::new(q)),
        transcodings: Arc::new(AtomicUsize::new(0)),
        max_transcodings: config.max_transcodings
        }
    };
    let mut server = HttpServer::new().bind(&config.local_addr, factory)?;
    server.no_proto();
    info!("Server listening on {}", server.local_addr().unwrap());
    server.run()?;


    Ok(())
}
fn main() {
    let config=match parse_args() {
        Err(e) => {
            writeln!(&mut io::stderr(), "Arguments error: {}",e).unwrap();
            std::process::exit(1)
        }
        Ok(c) => c
    };
    
    pretty_env_logger::init().unwrap();
    debug!("Started with following config {:?}", config);
    start_server(config).unwrap();
}
