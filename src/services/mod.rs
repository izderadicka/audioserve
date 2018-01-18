use hyper::server::{NewService, Request, Response, Service};
use hyper::{Method, StatusCode};
use hyper::header::{Range,AccessControlAllowOrigin, AccessControlAllowCredentials, 
Origin};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use self::subs::{send_file, send_file_simple, short_response_boxed, search,ResponseFuture, 
    NOT_FOUND_MESSAGE, get_folder};
use std::path::{PathBuf, Path};
use percent_encoding::percent_decode;
use futures::{Future, future};
use self::auth::Authenticator;
use self::search::Search;
use self::transcode::Transcoder;
use url::form_urlencoded;
use std::collections::HashMap;

mod subs;
mod types;
pub mod search;
pub mod auth;
pub mod transcode;

const OVERLOADED_MESSAGE: &str = "Overloaded, try later";

type Counter = Arc<AtomicUsize>;



pub struct Factory {
    pub sending_threads: Counter,
    pub max_threads: usize,
    pub base_dir: PathBuf,
    pub client_dir: PathBuf,
    pub authenticator: Arc<Box<Authenticator<Credentials=()>>>,
    pub search: Search,
    pub transcoding: TranscodingDetails,
    pub cors: bool
    
}

impl NewService for Factory {
    type Request = Request;
    type Response = Response;
    type Error = ::hyper::Error;
    type Instance = FileSendService;

    fn new_service(&self) -> Result<Self::Instance, io::Error> {
        Ok(FileSendService {
            authenticator: self.authenticator.clone(),
            sending_threads: self.sending_threads.clone(),
            max_threads: self.max_threads,
            base_dir: self.base_dir.clone(),
            search: self.search.clone(),
            transcoding: self.transcoding.clone(),
            client_dir: self.client_dir.clone(),
            cors: self.cors.clone()
        })
    }
}

#[derive(Clone)]
pub struct TranscodingDetails {
    pub transcoder: Option<Transcoder>,
    pub transcodings: Counter,
    pub max_transcodings: usize

}

pub struct FileSendService {
    pub authenticator: Arc<Box<Authenticator<Credentials=()>>>,
    search: Search,
    base_dir: PathBuf,
    client_dir: PathBuf,
    sending_threads: Counter,
    max_threads: usize,
    transcoding: TranscodingDetails,
    cors: bool
}

// use only on checked prefixes
fn get_subfolder(path: &str, prefix: &str) -> PathBuf {
    Path::new(&path).strip_prefix(prefix).unwrap().to_path_buf()
}

fn add_cors_headers(resp: Response, origin: Option<String>, enabled: bool) -> Response {
    if ! enabled {
        return resp
    }
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
        //static files 
        if req.path() == "/" {
            return send_file_simple(self.client_dir.clone(), "index.html".into(), self.sending_threads.clone());
        };
        if req.path() =="/bundle.js" {
            return send_file_simple(self.client_dir.clone(), "bundle.js".into(), self.sending_threads.clone());
        }
        // from here everything must be authenticated
        let base_dir = self.base_dir.clone();
        let sending_threads =  self.sending_threads.clone();
        let searcher = self.search.clone();
        let transcoding = self.transcoding.clone();
        let cors = self.cors;
        let origin = req.headers().get::<Origin>().map(|o| {
            format!("{}",o)
            }
            );
        Box::new(self.authenticator.authenticate(req).and_then(move |result| {
            match result {
                Ok((req,_creds)) => 
                    FileSendService::process_checked(req, base_dir,sending_threads, searcher, transcoding),
                Err(resp) => Box::new(future::ok(resp))
            }.map(move |r| add_cors_headers(r, origin, cors))
        }))
        
    }
}

impl FileSendService {
    fn process_checked(req: Request, 
        base_dir: PathBuf, 
        sending_threads: 
        Counter, searcher: Search,
        transcoding: TranscodingDetails
        ) -> ResponseFuture {
        
        let params =  req.query().map(|query| form_urlencoded::parse(query.as_bytes())
                            .collect::<HashMap<_, _>>());
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
                let seek: Option<f32> = params.and_then(|mut p| p.remove("seek")).and_then(|s| s.parse().ok());

                send_file(base_dir, 
                    get_subfolder(&path, "/audio/"), 
                    bytes_range, 
                    seek,
                    sending_threads,
                    transcoding,
                    )
                } else if path.starts_with("/folder/") {
                    get_folder(base_dir, 
                    get_subfolder(&path, "/folder/"),  
                    transcoding.transcoder,
                    sending_threads) 
                } else if path == "/search" {
                    if let Some(search_string) = params.and_then(|mut p| p.remove("q")){
                        return search(base_dir, searcher, search_string.into_owned(), sending_threads);
                    }
                    short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE)
                } else {
                    short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE)
                }
            },

            _ => short_response_boxed(StatusCode::MethodNotAllowed, "Method not supported"),
        }
    }
}
