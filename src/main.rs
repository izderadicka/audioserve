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

use hyper::server::{Http as HttpServer, NewService, Request, Response, Service};
use hyper::{Method, StatusCode};
use hyper::header::{Range};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use subs::{send_file, short_response_boxed, ResponseFuture, NOT_FOUND_MESSAGE, get_folder};
mod subs;
mod types;


const OVERLOADED_MESSAGE: &str = "Overloaded, try later";
const MAX_SENDING_THREADS: usize = 10;

type Counter = Arc<AtomicUsize>;

struct Factory {
    sending_threads: Counter,
}

impl NewService for Factory {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Instance = FileSendService;

    fn new_service(&self) -> Result<Self::Instance, io::Error> {
        Ok(FileSendService {
            sending_threads: self.sending_threads.clone(),
        })
    }
}
struct FileSendService {
    sending_threads: Counter,
}


impl Service for FileSendService {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = ResponseFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        if self.sending_threads.load(Ordering::SeqCst) > MAX_SENDING_THREADS {
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
                let base_path = "./".into();
                get_folder(base_path, "test_data".into(),  self.sending_threads.clone()) 
            }

            (_, _) => short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE),
        }
    }
}

fn start_server() -> Result<(), hyper::Error> {
    let addr = "127.0.0.1:3000".parse().unwrap();
    let factory = Factory {
        sending_threads: Arc::new(AtomicUsize::new(0)),
    };
    let mut server = HttpServer::new().bind(&addr, factory)?;
    server.no_proto();
    info!("Server listening on {}", server.local_addr().unwrap());
    server.run()?;


    Ok(())
}
fn main() {
    pretty_env_logger::init().unwrap();
    start_server().unwrap();
}
