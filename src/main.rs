extern crate futures;
extern crate hyper;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;
extern crate futures_cpupool;

use hyper::server::{Http as HttpServer, Request, Response, Service, NewService};
use hyper::{Chunk, Method, StatusCode};
use hyper::header::{ContentLength, ContentType};
use futures::future::{self, Future};
use futures::sync::{mpsc, oneshot};
use futures::Sink;
use std::io::{self, Read};
use std::fs::File;
use std::thread;
use futures_cpupool::{CpuPool, CpuFuture};
use std::rc::Rc;

const NOT_FOUND_MESSAGE: &str = "Not Found";
const THREAD_SEND_ERROR: &str = "Cannot communicate with other thread";
const BUF_SIZE: usize = 8 * 1024;

type ResponseFuture = Box<Future<Item = Response, Error = hyper::Error>>;

struct Factory(Rc<CpuPool>);

impl NewService for Factory {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Instance = FileSendService;

    fn new_service(&self) -> Result<Self::Instance, io::Error> {
        Ok(FileSendService(self.0.clone()))
    }
}
struct FileSendService(Rc<CpuPool>);

fn send_file(file_path: &'static str, pool: &CpuPool) -> ResponseFuture {
    let (tx, rx) = oneshot::channel();

    let r: CpuFuture<(),()> = pool
    .spawn_fn(move || {
        let mut file = match File::open(file_path) {
            Ok(f) => f,
            Err(_e) => {
                tx.send(
                    Response::new()
                        .with_body(NOT_FOUND_MESSAGE)
                        .with_header(ContentLength(NOT_FOUND_MESSAGE.len() as u64))
                        .with_status(StatusCode::NotFound),
                ).expect(THREAD_SEND_ERROR);
                return Ok(());
            }
        };
        let (mut body_tx, body_rx) = mpsc::channel(1);
        let sz = file.metadata().map(|m| m.len());
        let mut res = Response::new()
            .with_body(body_rx)
            .with_header(ContentType("audio/ogg".parse().unwrap()));
        if let Ok(file_sz) = sz {
            res = res.with_header(ContentLength(file_sz as u64));
        }
        tx.send(res).expect(THREAD_SEND_ERROR);
        let mut buf = [0u8; BUF_SIZE];
        loop {
            match file.read(&mut buf) {
                Ok(n) => if n == 0 {
                    body_tx.close().expect(THREAD_SEND_ERROR);
                    break;
                } else {
                    let c: Chunk = buf.to_vec().into();
                    match body_tx.send(Ok(c)).wait() {
                        Ok(t) => body_tx = t,
                        Err(_) => break,
                    };
                },
                Err(_) => break,
            }
        };
        Ok(())
    });
    // We should keep task running even after it's reference is gone
    r.forget();
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
                send_file("test_data/julie.opus", &self.0)
            }

            (_, _) => Box::new(future::ok(
                Response::new()
                    .with_status(StatusCode::NotFound)
                    .with_body("Not Found"),
            )),
        }
    }
}

fn start_server() -> Result<(), hyper::Error> {
    let addr = "127.0.0.1:3000".parse().unwrap();
    let factory = Factory(Rc::new(CpuPool::new(10000)));
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
