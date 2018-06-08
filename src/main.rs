#[macro_use]
extern crate clap;
extern crate data_encoding;
extern crate futures;
#[macro_use]
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
extern crate regex;
extern crate serde_json;
extern crate serde_yaml;
extern crate taglib;
extern crate url;
#[macro_use]
extern crate lazy_static;
extern crate simple_thread_pool;
// for TLS
#[cfg(feature = "tls")]
extern crate native_tls;
#[cfg(feature = "tls")]
extern crate tokio_proto;
#[cfg(feature = "tls")]
extern crate tokio_tls;

use config::{get_config, parse_args};
use hyper::server::Http as HttpServer;
use ring::rand::{SecureRandom, SystemRandom};
use services::auth::SharedSecretAuthenticator;
use services::search::Search;
use services::{FileSendService, TranscodingDetails};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

#[cfg(feature = "tls")]
use self::tokio_proto::TcpServer;
#[cfg(feature = "tls")]
use native_tls::{Pkcs12, TlsAcceptor};
#[cfg(feature = "tls")]
use tokio_tls::proto;

mod config;
mod services;

#[cfg(feature = "tls")]
fn load_private_key<P>(file: P, pass: Option<&String>) -> Result<Pkcs12, io::Error>
where
    P: AsRef<Path>,
{
    let mut bytes = vec![];
    let mut f = File::open(file)?;
    f.read_to_end(&mut bytes)?;
    let key = Pkcs12::from_der(&bytes, pass.unwrap_or(&String::new()))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(key)
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

fn start_server(my_secret: Vec<u8>) -> Result<(), Box<std::error::Error>> {
    let cfg = get_config();
    let svc = FileSendService {
        pool: simple_thread_pool::Builder::new()
            .set_max_queue(cfg.pool_size.queue_size)
            .set_min_threads(cfg.pool_size.min_threads)
            .set_max_threads(cfg.pool_size.max_threads)
            .build(),
        authenticator: get_config().shared_secret.as_ref().map(
            |secret| -> Arc<Box<services::auth::Authenticator<Credentials = ()>>> {
                Arc::new(Box::new(SharedSecretAuthenticator::new(
                    secret.clone(),
                    my_secret,
                    cfg.token_validity_hours,
                )))
            },
        ),
        search: Search::FoldersSearch,
        transcoding: TranscodingDetails {
            transcodings: Arc::new(AtomicUsize::new(0)),
            max_transcodings: cfg.max_transcodings,
        },
    };

    match get_config().ssl_key_file.as_ref() {
        None => {
            let server = HttpServer::new().bind(&get_config().local_addr, move || Ok(svc.clone()))?;
            //server.no_proto();
            info!("Server listening on {}", server.local_addr().unwrap());
            server.run()?;
        }
        Some(file) => {
            #[cfg(feature = "tls")]
            {
                let private_key =
                    match load_private_key(file, get_config().ssl_key_password.as_ref()) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Error loading SSL/TLS private key: {}", e);
                            return Err(Box::new(e));
                        }
                    };
                let tls_cx = TlsAcceptor::builder(private_key)?.build()?;
                let proto = proto::Server::new(HttpServer::new(), tls_cx);

                let addr = cfg.local_addr;
                let srv = TcpServer::new(proto, addr);
                println!("TLS Listening on {}", addr);
                srv.serve(move || Ok(svc.clone()));
            }

            #[cfg(not(feature = "tls"))]
            {
                panic!(
                    "TLS is not compiled - build with default features {:?}",
                    file
                )
            }
        }
    }
    Ok(())
}

fn main() {
    match parse_args() {
        Err(e) => {
            writeln!(&mut io::stderr(), "Arguments error: {}", e).unwrap();
            process::exit(1)
        }
        Ok(c) => c,
    };
    pretty_env_logger::init();
    debug!("Started with following config {:?}", get_config());
    let my_secret = match gen_my_secret(&get_config().secret_file) {
        Ok(s) => s,
        Err(e) => {
            error!("Error creating/reading secret: {}", e);
            process::exit(2)
        }
    };

    match start_server(my_secret) {
        Ok(_) => (),
        Err(e) => {
            error!("Error starting server: {}", e);
            process::exit(3)
        }
    }
}
