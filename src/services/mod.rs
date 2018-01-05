use hyper::server::{NewService, Request, Response, Service};
use hyper::{Method, StatusCode};
use hyper::header::{Range,AccessControlAllowOrigin};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use self::subs::{send_file, short_response_boxed, ResponseFuture, NOT_FOUND_MESSAGE, get_folder};
use std::path::{PathBuf, Path};
use percent_encoding::percent_decode;
use futures::Future;

mod subs;
mod types;


const OVERLOADED_MESSAGE: &str = "Overloaded, try later";

type Counter = Arc<AtomicUsize>;

pub struct Factory {
    pub sending_threads: Counter,
    pub max_threads: usize,
    pub base_dir: PathBuf
}

impl NewService for Factory {
    type Request = Request;
    type Response = Response;
    type Error = ::hyper::Error;
    type Instance = FileSendService;

    fn new_service(&self) -> Result<Self::Instance, io::Error> {
        Ok(FileSendService {
            sending_threads: self.sending_threads.clone(),
            max_threads: self.max_threads,
            base_dir: self.base_dir.clone()
        })
    }
}
pub struct FileSendService {
    sending_threads: Counter,
    max_threads: usize,
    base_dir: PathBuf
}

// use only on checked prefixes
fn get_subfolder(path: &str, prefix: &str) -> PathBuf {
    Path::new(&path).strip_prefix(prefix).unwrap().to_path_buf()
}

fn add_cors_headers(resp: Response) -> Response {
    resp.with_header(AccessControlAllowOrigin::Any)
}

impl Service for FileSendService {
    type Request = Request;
    type Response = Response;
    type Error = ::hyper::Error;
    type Future = ResponseFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        if self.sending_threads.load(Ordering::SeqCst) > self.max_threads {
                    warn!("Server is busy, refusing request");
                    return short_response_boxed(
                        StatusCode::ServiceUnavailable,
                        OVERLOADED_MESSAGE,
                    );
        };

        let response = match req.method() {
            &Method::Get => {
                let path = percent_decode(req.path().as_bytes()).decode_utf8_lossy().into_owned();
                
                if path.starts_with("/audio/") {
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

                send_file(self.base_dir.clone(), 
                    get_subfolder(&path, "/audio/"), 
                    bytes_range, 
                    self.sending_threads.clone())
                } else if path.starts_with("/folder/") {
                    get_folder(self.base_dir.clone(), 
                    get_subfolder(&path, "/folder/"),  
                    self.sending_threads.clone()) 
                } else {
                    short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE)
                }
            },

            _ => short_response_boxed(StatusCode::MethodNotAllowed, "Method not supported"),
        };

        Box::new(response.map(|r| add_cors_headers((r))))
    }
}
