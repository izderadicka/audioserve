use hyper::server::{NewService, Request, Response, Service};
use hyper::{Method, StatusCode};
use hyper::header::{Range,AccessControlAllowOrigin, AccessControlAllowCredentials, 
Origin};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use self::subs::{send_file, short_response_boxed, search,ResponseFuture, NOT_FOUND_MESSAGE, get_folder};
use std::path::{PathBuf, Path};
use percent_encoding::percent_decode;
use futures::{Future, future};
use self::auth::Authenticator;
use self::search::Search;
use url::form_urlencoded;
use std::collections::HashMap;

mod subs;
mod types;
pub mod search;
pub mod auth;

const OVERLOADED_MESSAGE: &str = "Overloaded, try later";

type Counter = Arc<AtomicUsize>;



pub struct Factory {
    pub sending_threads: Counter,
    pub max_threads: usize,
    pub base_dir: PathBuf,
    pub authenticator: Arc<Box<Authenticator<Credentials=()>>>,
    pub search: Search
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
            base_dir: self.base_dir.clone(),
            authenticator: self.authenticator.clone(),
            search: self.search.clone()
        })
    }
}
pub struct FileSendService {
    sending_threads: Counter,
    max_threads: usize,
    base_dir: PathBuf,
    pub authenticator: Arc<Box<Authenticator<Credentials=()>>>,
    search: Search
}

// use only on checked prefixes
fn get_subfolder(path: &str, prefix: &str) -> PathBuf {
    Path::new(&path).strip_prefix(prefix).unwrap().to_path_buf()
}

fn add_cors_headers(resp: Response, origin: Option<String>) -> Response {
    match origin {
        Some(o) =>
            resp.with_header(AccessControlAllowOrigin::Value(o))
            .with_header(AccessControlAllowCredentials),
        None => resp
    }
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
        let base_dir = self.base_dir.clone();
        let sending_threads =  self.sending_threads.clone();
        let searcher = self.search.clone();
        let origin = req.headers().get::<Origin>().map(|o| {
            format!("{}",o)
            }
            );
        Box::new(self.authenticator.authenticate(req).and_then(move |result| {
            match result {
                Ok((req,_creds)) => FileSendService::process_checked(req, base_dir,sending_threads, searcher),
                Err(resp) => Box::new(future::ok(resp))
            }.map(|r| add_cors_headers(r, origin))
        }))
        
    }
}

impl FileSendService {
    fn process_checked(req: Request, base_dir: PathBuf, sending_threads: Counter, searcher: Search) -> ResponseFuture {

        match req.method() {
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

                send_file(base_dir, 
                    get_subfolder(&path, "/audio/"), 
                    bytes_range, 
                    sending_threads)
                } else if path.starts_with("/folder/") {
                    get_folder(base_dir, 
                    get_subfolder(&path, "/folder/"),  
                    sending_threads) 
                } else if path == "/search" {
                    if let Some(query) = req.query() {
                        let mut params = form_urlencoded::parse(query.as_bytes())
                            .collect::<HashMap<_, _>>();
                        if let Some(search_string) = params.remove("q"){
                            return search(base_dir, searcher, search_string.into_owned(), sending_threads);
                        }
                        
                    };
                    short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE)
                } else {
                    short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE)
                }
            },

            _ => short_response_boxed(StatusCode::MethodNotAllowed, "Method not supported"),
        }
    }
}
