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
use std::io::{self, Write, Read};
use std::sync::atomic::{AtomicUsize};
use std::sync::Arc;
use services::{Factory, TranscodingDetails};
use services::auth::SharedSecretAuthenticator;
use services::search::Search;
use services::transcode::Transcoder;
use config::{parse_args, Config};
use ring::rand::{SecureRandom,SystemRandom};
use std::path::Path;
use std::fs::File;
use std::process;

mod services;
mod config;


fn gen_my_secret<P: AsRef<Path>>(file: P) -> Result<Vec<u8>, io::Error> {

let file = file.as_ref();
if file.exists() {
    let mut v = vec![];
    let size = file.metadata()?.len();
    if size > 128 {
        return Err(io::Error::new(io::ErrorKind::Other, "Secret too long"));
    }

    let mut f = File::open(file)?;
    f.read_to_end(&mut v)?;
    Ok(v)
} else {
    let mut random = [0u8; 32];
    let rng = SystemRandom::new();
    rng.fill(&mut random).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut f = File::create(file)?;
    f.write_all(&random)?;
    Ok(random.iter().cloned().collect())

}

}

fn start_server(config: Config, my_secret: Vec<u8>) -> Result<(), hyper::Error> {
    
    let factory = Factory {
        sending_threads: Arc::new(AtomicUsize::new(0)),
        max_threads: config.max_sending_threads,
        base_dir: config.base_dir,
        client_dir: config.client_dir,
        authenticator: Arc::new(Box::new(SharedSecretAuthenticator::new(
            config.shared_secret,
            my_secret,
            config.token_validity_hours
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
            process::exit(1)
        }
        Ok(c) => c
    };
    pretty_env_logger::init().unwrap();
    debug!("Started with following config {:?}", config);
    let my_secret =  match gen_my_secret(&config.secret_file) {
        Ok(s) => s,
        Err(e) => {
            error!("Error creating/reading secret: {}", e);
            process::exit(2)
        }
    };

    match start_server(config, my_secret) {
        Ok(_) => (),
        Err(e) => {
            error!("Error starting server: {}",e);
            process::exit(3)
        }
    }
}
