#[macro_use]
extern crate clap;
extern crate data_encoding;
extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate mime;
extern crate mime_guess;
extern crate num_cpus;
extern crate percent_encoding;
extern crate pretty_env_logger;
#[macro_use]
extern crate quick_error;
extern crate ring;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate taglib;
extern crate url;
// for TLS
extern crate native_tls;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_tls;


use hyper::server::Http as HttpServer;
use std::io::{self, Read, Write};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use services::{FileSendService, TranscodingDetails};
use services::auth::SharedSecretAuthenticator;
use services::search::Search;
use services::transcode::Transcoder;
use config::{parse_args, Config};
use ring::rand::{SecureRandom, SystemRandom};
use std::path::Path;
use std::fs::File;
use std::process;

use native_tls::{Pkcs12, TlsAcceptor};
use self::tokio_proto::TcpServer;
use tokio_tls::proto;

mod services;
mod config;

fn load_private_key<P>(file: Option<P>, pass: Option<&String>) -> Result<Option<Pkcs12>, io::Error> 
where P:AsRef<Path>
{
    match file {
        Some(fname) => {
            let mut bytes = vec![];
            let mut f = File::open(fname)?;
            f.read_to_end(&mut bytes)?;
            let key = Pkcs12::from_der(&bytes, pass.unwrap()).map_err(|e| 
            io::Error::new(io::ErrorKind::Other, e))?;
            Ok(Some(key))
        },
        None => Ok(None)
    }
    
}

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
        rng.fill(&mut random)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let mut f = File::create(file)?;
        f.write_all(&random)?;
        Ok(random.iter().cloned().collect())
    }
}

fn start_server(config: Config, my_secret: Vec<u8>, private_key: Option<Pkcs12>) -> Result<(), Box<std::error::Error>> {
    let svc = FileSendService {
        sending_threads: Arc::new(AtomicUsize::new(0)),
        max_threads: config.max_sending_threads,
        base_dir: config.base_dir,
        client_dir: config.client_dir,
        authenticator: Arc::new(Box::new(SharedSecretAuthenticator::new(
            config.shared_secret,
            my_secret,
            config.token_validity_hours,
        ))),
        search: Search::FoldersSearch,
        transcoding: TranscodingDetails {
            transcoder: config.transcoding.map(|q| Transcoder::new(q)),
            transcodings: Arc::new(AtomicUsize::new(0)),
            max_transcodings: config.max_transcodings,
        },
        cors: config.cors,
    };

    match private_key {
        None => {
            let server = HttpServer::new().bind(&config.local_addr, move || Ok(svc.clone()))?;
            //server.no_proto();
            info!("Server listening on {}", server.local_addr().unwrap());
            server.run()?;
        },
        Some(pk) => {
            let tls_cx = TlsAcceptor::builder(pk)?.build()?;
            let proto = proto::Server::new(HttpServer::new(), tls_cx);

        let addr = config.local_addr;
        let srv = TcpServer::new(proto, addr);
        println!("TLS Listening on {}", addr);
        srv.serve( move || Ok(svc.clone()));
        }
    }


    Ok(())
}
fn main() {
    let config = match parse_args() {
        Err(e) => {
            writeln!(&mut io::stderr(), "Arguments error: {}", e).unwrap();
            process::exit(1)
        }
        Ok(c) => c,
    };
    pretty_env_logger::init().unwrap();
    debug!("Started with following config {:?}", config);
    let my_secret = match gen_my_secret(&config.secret_file) {
        Ok(s) => s,
        Err(e) => {
            error!("Error creating/reading secret: {}", e);
            process::exit(2)
        }
    };


    let private_key = match load_private_key(config.ssl_key_file.as_ref(), config.ssl_key_password.as_ref()) {
        Ok(s) => s,
        Err(e) => {
            error!("Error loading SSL/TLS private key: {}", e);
            process::exit(3)
        }
    };

    match start_server(config, my_secret, private_key) {
        Ok(_) => (),
        Err(e) => {
            error!("Error starting server: {}", e);
            process::exit(3)
        }
    }
}
