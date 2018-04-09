use hyper::server::{Request, Response, Service};
use hyper::{Method, StatusCode};
use hyper::header::{Range,AccessControlAllowOrigin, AccessControlAllowCredentials, 
Origin};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use self::subs::{send_file, send_file_simple, short_response_boxed, search,ResponseFuture, 
    NOT_FOUND_MESSAGE, get_folder, collections_list, transcodings_list};
use std::path::{PathBuf, Path};
use percent_encoding::percent_decode;
use futures::{Future, future};
use self::auth::Authenticator;
use self::search::Search;
use url::form_urlencoded;
use std::collections::HashMap;
use config::get_config;
use regex::Regex;
use self::transcode::QualityLevel;

mod subs;
mod types;
pub mod search;
pub mod auth;
pub mod transcode;

const APP_STATIC_FILES_CACHE_AGE: u32 = 30*24*3600;
const FOLDER_INFO_FILES_CACHE_AGE: u32 = 24*3600;

const OVERLOADED_MESSAGE: &str = "Overloaded, try later";
lazy_static! {
    static ref COLLECTION_NUMBER_RE: Regex = Regex::new(r"^/(\d+)/.+").unwrap();
}

type Counter = Arc<AtomicUsize>;


#[derive(Clone)]
pub struct TranscodingDetails {
    pub transcodings: Counter,
    pub max_transcodings: usize

}


#[derive(Clone)]
pub struct FileSendService {
   pub authenticator: Option<Arc<Box<Authenticator<Credentials=()>>>>,
   pub  search: Search,
   pub  sending_threads: Counter,
   pub  transcoding: TranscodingDetails,
}

// use only on checked prefixes
fn get_subpath(path: &str, prefix: &str) -> PathBuf {
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
        if self.sending_threads.load(Ordering::SeqCst) > get_config().max_transcodings {
                    warn!("Server is busy, refusing request");
                    return short_response_boxed(
                        StatusCode::ServiceUnavailable,
                        OVERLOADED_MESSAGE,
                    );
        };
        //static files 
        if req.path() == "/" {
            return send_file_simple(&get_config().client_dir, 
            "index.html".into(), 
            Some(APP_STATIC_FILES_CACHE_AGE),
            self.sending_threads.clone());
        };
        if req.path() =="/bundle.js" {
            return send_file_simple(&get_config().client_dir, 
            "bundle.js".into(), 
            Some(APP_STATIC_FILES_CACHE_AGE),
            self.sending_threads.clone());
        }
        // from here everything must be authenticated
        let sending_threads =  self.sending_threads.clone();
        let searcher = self.search.clone();
        let transcoding = self.transcoding.clone();
        let cors = get_config().cors;
        let origin = req.headers().get::<Origin>().map(|o| {
            format!("{}",o)
            }
            );

        let resp = match self.authenticator {

        Some(ref auth) =>
        Box::new(auth.authenticate(req).and_then(move |result| {
            match result {
                Ok((req,_creds)) => 
                    FileSendService::process_checked(req, sending_threads, searcher, transcoding),
                Err(resp) => Box::new(future::ok(resp))
            }
        }
        )),
        None => FileSendService::process_checked(req, sending_threads, searcher, transcoding)

        };
        Box::new(resp.map(move |r| add_cors_headers(r, origin, cors)))
    }
}

impl FileSendService {
    fn process_checked(req: Request, 
        sending_threads: 
        Counter, searcher: Search,
        transcoding: TranscodingDetails
        ) -> ResponseFuture {
        
        let mut params =  req.query().map(|query| form_urlencoded::parse(query.as_bytes())
                            .collect::<HashMap<_, _>>());
        match req.method() {
            &Method::Get => {
                
                let mut path = percent_decode(req.path().as_bytes()).decode_utf8_lossy().into_owned();

                if path.starts_with("/collections") {
                    collections_list() 
                } else if path.starts_with("/transcodings") {
                    transcodings_list()
                } else {
                // TODO -  select correct base dir
                let mut colllection_index = 0;
                let mut new_path: Option<String> = None;

                {
                let matches = COLLECTION_NUMBER_RE.captures(&path);
                    if matches.is_some() {
                        let cnum = matches.unwrap().get(1).unwrap();
                        // match gives us char position is it's safe to slice
                        new_path = Some((&path[cnum.end()..]).to_string());
                        // and cnum is guarateed to contain digits only
                        let cnum: usize = cnum.as_str().parse().unwrap();
                        if cnum >= get_config().base_dirs.len() {
                            return short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE)
                        }
                        colllection_index = cnum;
                    }
                }
                if new_path.is_some() {
                    path = new_path.unwrap();
                }
                let base_dir = &get_config().base_dirs[colllection_index];
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
                let seek: Option<f32> = params.as_mut().and_then(|p| p.remove("seek")).and_then(|s| s.parse().ok());
                let transcoding_quality: Option<QualityLevel> = params.and_then(|mut p| p.remove("trans"))
                    .and_then(|t| QualityLevel::from_letter(&t));

                send_file(base_dir, 
                    get_subpath(&path, "/audio/"), 
                    bytes_range, 
                    seek,
                    sending_threads,
                    transcoding,
                    transcoding_quality
                    )
                } else if path.starts_with("/folder/") {
                    get_folder(base_dir, 
                    get_subpath(&path, "/folder/"),  
                    sending_threads) 
                } else if path == "/search" {
                    if let Some(search_string) = params.and_then(|mut p| p.remove("q")){
                        return search(base_dir, searcher, search_string.into_owned(), sending_threads);
                    }
                    short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE)
                } else if path.starts_with("/cover/") {
                    send_file_simple(base_dir, 
                    get_subpath(&path, "/cover"), 
                    Some(FOLDER_INFO_FILES_CACHE_AGE),
                    sending_threads)

                } else if path.starts_with("/desc/") {
                    send_file_simple(base_dir, 
                    get_subpath(&path, "/desc"), 
                    Some(FOLDER_INFO_FILES_CACHE_AGE),
                    sending_threads)
                } else {
                    short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE)
                }
                }
            },

            _ => short_response_boxed(StatusCode::MethodNotAllowed, "Method not supported"),
        }
    }
}
