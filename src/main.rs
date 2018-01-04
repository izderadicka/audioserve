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

use hyper::server::{Http as HttpServer, NewService, Request, Response, Service};
use hyper::{Method, StatusCode};
use hyper::header::{Range};
use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use subs::{send_file, short_response_boxed, ResponseFuture, NOT_FOUND_MESSAGE, get_folder};
use config::{parse_args, Config};
use std::path::PathBuf;

mod subs;
mod types;
mod config;


const OVERLOADED_MESSAGE: &str = "Overloaded, try later";

type Counter = Arc<AtomicUsize>;

struct Factory {
    sending_threads: Counter,
    max_threads: usize,
    base_dir: PathBuf
}

impl NewService for Factory {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Instance = FileSendService;

    fn new_service(&self) -> Result<Self::Instance, io::Error> {
        Ok(FileSendService {
            sending_threads: self.sending_threads.clone(),
            max_threads: self.max_threads,
            base_dir: self.base_dir.clone()
        })
    }
}
struct FileSendService {
    sending_threads: Counter,
    max_threads: usize,
    base_dir: PathBuf
}


impl Service for FileSendService {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = ResponseFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        if self.sending_threads.load(Ordering::SeqCst) > self.max_threads {
                    warn!("Server is busy, refusing request");
                    return short_response_boxed(
                        StatusCode::ServiceUnavailable,
                        OVERLOADED_MESSAGE,
                    );
        }
        match (req.method(), req.path()) {
            (&Method::Get, "/audio") => {
                debug!("Received request with following headers {}", req.headers());

                let range = req.headers().get::<Range>();
                let bytes_range = match range {
                    Some(&Range::Bytes(ref bytes_ranges)) => {
                        if bytes_ranges.len() < 1 {
                            return short_response_boxed(StatusCode::BadRequest, "One range is required")
                        } else if bytes_ranges.len() > 1 {
                            return short_response_boxed(StatusCode::NotImplemented, "Do not support muptiple ranges")
                        } else {
                            Some(bytes_ranges[0].clone())
                        }
                    },
                    Some(_) => return short_response_boxed(StatusCode::NotImplemented, 
                    "Other then bytes ranges are not supported"),
                    None => None
                };

                send_file("test_data/julie.opus".into(), bytes_range, self.sending_threads.clone())
            },
            (&Method::Get, "/folder") => {
                
                get_folder(self.base_dir.clone(), "".into(),  self.sending_threads.clone()) 
            }

            (_, _) => short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE),
        }
    }
}

fn start_server(config: Config) -> Result<(), hyper::Error> {
    
    let factory = Factory {
        sending_threads: Arc::new(AtomicUsize::new(0)),
        max_threads: config.max_sending_threads,
        base_dir: config.base_dir
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
    debug!("Started with following config {:?}", config);
    pretty_env_logger::init().unwrap();

    start_server(config).unwrap();
}
