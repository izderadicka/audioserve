extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;

use hyper::server::{Http as HttpServer, NewService, Request, Response, Service};
use hyper::{Chunk, Method, StatusCode};
use hyper::header::{ContentLength, ContentType, AcceptRanges, RangeUnit, Range, 
ContentRange, ContentRangeSpec};
use futures::future::{Future};
use futures::sync::{mpsc, oneshot};
use futures::Sink;
use std::io::{self, Read, Seek, SeekFrom};
use std::fs::File;
use std::thread;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use utils::{short_response, ResponseFuture, short_response_boxed};

mod utils;

const NOT_FOUND_MESSAGE: &str = "Not Found";
const OVERLOADED_MESSAGE: &str = "Overloaded, try later";
const THREAD_SEND_ERROR: &str = "Cannot communicate with other thread";
const BUF_SIZE: usize = 8 * 1024;
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



fn send_file(file_path: &'static str, 
range: Option<hyper::header::ByteRangeSpec>, 
counter: Counter) -> ResponseFuture {
    let (tx, rx) = oneshot::channel();
    counter.fetch_add(1, Ordering::SeqCst);
    thread::spawn(move || {
        match File::open(file_path) {
            Ok(mut file) => {
                let (mut body_tx, body_rx) = mpsc::channel(1);
                let file_sz = file.metadata().map(|m| m.len()).expect("File stat error");
                
                let mut res = Response::new()
                    .with_body(body_rx)
                    .with_header(ContentType("audio/ogg".parse().unwrap()));
                let range = match range {
                    Some(r) => 
                        match r.to_satisfiable_range(file_sz) {
                            Some((s,e)) => {
                                assert!(e>=s);
                                Some((s, e, e-s+1))
                            },
                            None => None
                        },
                    None => None
                };
               
                    
                let (start, content_len) = match range {
                    Some((s,e,l)) => {
                        
                        res = res.with_header(ContentRange(ContentRangeSpec::Bytes{
                                    range:Some((s,e)),
                                    instance_length: Some(file_sz)
                                    }))
                                    .with_status(StatusCode::PartialContent);
                            (s, l)
                            },
                        None => {
                            res=res.with_header(AcceptRanges(vec![RangeUnit::Bytes]));
                            (0,file_sz)
                        }
                };
                   
                
                res = res.with_header(ContentLength(content_len));
                
                tx.send(res).expect(THREAD_SEND_ERROR);
                let mut buf = [0u8; BUF_SIZE];
                if start>0 {
                    file.seek(SeekFrom::Start(start)).expect("Seek error");
                }
                let mut remains = content_len as usize;
                loop {
                    match file.read(&mut buf) {
                        Ok(n) => if n == 0 {
                            trace!("Received 0");
                            body_tx.close().expect(THREAD_SEND_ERROR);
                            break;
                        } else {
                            let to_send = n.min(remains);
                            trace!("Received {}, remains {}, sending {}", n, remains, to_send);
                            let slice = buf[..to_send].to_vec();
                            let c: Chunk = slice.into();
                            match body_tx.send(Ok(c)).wait() {
                                Ok(t) => body_tx = t,
                                Err(_) => break,
                            };

                            if remains <= n {
                                trace!("All send");
                                body_tx.close().expect(THREAD_SEND_ERROR);
                                break;
                            } else {
                                remains -= n
                            }
                        },
                        
                        Err(e) => {
                            error!("Sending file error {}", e);
                            break
                        },
                    }
                }
            }
            Err(e) => {
                error!("File opening error {}", e);
                tx.send(short_response(StatusCode::NotFound, NOT_FOUND_MESSAGE))
                    .expect(THREAD_SEND_ERROR);
            }
        };
        counter.fetch_sub(1, Ordering::SeqCst);
    });
    Box::new(rx.map_err(|e| {
        hyper::Error::from(io::Error::new(io::ErrorKind::Other, e))
    }))
}
impl Service for FileSendService {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = ResponseFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        match (req.method(), req.path()) {
            (&Method::Get, "/audio") => {
                debug!("Received request with following headers {}", req.headers());
                if self.sending_threads.load(Ordering::SeqCst) > MAX_SENDING_THREADS {
                    return short_response_boxed(
                        StatusCode::ServiceUnavailable,
                        OVERLOADED_MESSAGE,
                    );
                }

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

                send_file("test_data/julie.opus", bytes_range, self.sending_threads.clone())
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
