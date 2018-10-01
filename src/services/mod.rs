use self::auth::Authenticator;
use self::search::Search;
use self::subs::{
    collections_list, get_folder, search, send_file, send_file_simple, short_response_boxed,
    transcodings_list, ResponseFuture, NOT_FOUND_MESSAGE,
};
use self::transcode::QualityLevel;
use config::get_config;
use futures::{future, Future};
use hyper::header::{
    HeaderValue, ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN, RANGE,
};
use hyper::service::Service;
use hyper::{Body, Method, Request, Response, StatusCode};
use hyperx::header::{Header, Range};
use percent_encoding::percent_decode;
use regex::Regex;
use std::collections::HashMap;
#[cfg(feature = "symlinks")]
use std::fs::read_link;
use std::fs::DirEntry;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use url::form_urlencoded;

pub mod auth;
pub mod search;
mod subs;
pub mod transcode;
mod types;

const APP_STATIC_FILES_CACHE_AGE: u32 = 30 * 24 * 3600;
const FOLDER_INFO_FILES_CACHE_AGE: u32 = 24 * 3600;

lazy_static! {
    static ref COLLECTION_NUMBER_RE: Regex = Regex::new(r"^/(\d+)/.+").unwrap();
}

type Counter = Arc<AtomicUsize>;

#[derive(Clone)]
pub struct TranscodingDetails {
    pub transcodings: Counter,
    pub max_transcodings: usize,
}

#[derive(Clone)]
pub struct FileSendService<T> {
    pub authenticator: Option<Arc<Box<Authenticator<Credentials = T>>>>,
    pub search: Search,
    pub transcoding: TranscodingDetails,
}

// use only on checked prefixes
fn get_subpath(path: &str, prefix: &str) -> PathBuf {
    Path::new(&path).strip_prefix(prefix).unwrap().to_path_buf()
}

fn add_cors_headers<T: AsRef<str>>(
    mut resp: Response<Body>,
    origin: Option<T>,
    enabled: bool,
) -> Response<Body> {
    if enabled {
        return resp;
    }
    match origin {
        Some(o) => {
            if let Ok(origin_value) = HeaderValue::from_str(o.as_ref()) {
                let headers = resp.headers_mut();
                headers.append(ACCESS_CONTROL_ALLOW_ORIGIN, origin_value);
                headers.append(
                    ACCESS_CONTROL_ALLOW_CREDENTIALS,
                    HeaderValue::from_static("true"),
                );
            }
            resp
        }
        None => resp,
    }
}

impl <C:'static>Service for FileSendService<C> {
    type ReqBody = Body;
    type ResBody = Body;
    type Error = ::error::Error;
    type Future = Box<Future<Item = Response<Self::ResBody>, Error = Self::Error> + Send>;
    fn call(&mut self, req: Request<Self::ReqBody>) -> Self::Future {
        //static files
        if req.uri().path() == "/" {
            return send_file_simple(
                &get_config().client_dir,
                "index.html",
                Some(APP_STATIC_FILES_CACHE_AGE),
            );
        };
        if req.uri().path() == "/bundle.js" {
            return send_file_simple(
                &get_config().client_dir,
                "bundle.js",
                Some(APP_STATIC_FILES_CACHE_AGE),
            );
        }
        // from here everything must be authenticated
        let searcher = self.search.clone();
        let transcoding = self.transcoding.clone();
        let cors = get_config().cors;
        let origin = req
            .headers()
            .get(ORIGIN)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_owned());

        let resp = match self.authenticator {
            Some(ref auth) => {
                Box::new(auth.authenticate(req).and_then(move |result| match result {
                    Ok((req, _creds)) => {
                        FileSendService::<C>::process_checked(&req, searcher, transcoding)
                    }
                    Err(resp) => Box::new(future::ok(resp)),
                }))
            }
            None => FileSendService::<C>::process_checked(&req, searcher, transcoding),
        };
        Box::new(resp.map(move |r| add_cors_headers(r, origin, cors)))
    }
}

impl <C> FileSendService<C> {
    fn process_checked<T>(
        req: &Request<T>,
        searcher: Search,
        transcoding: TranscodingDetails,
    ) -> ResponseFuture {
        let mut params = req
            .uri()
            .query()
            .map(|query| form_urlencoded::parse(query.as_bytes()).collect::<HashMap<_, _>>());
        match *req.method() {
            Method::GET => {
                let mut path = percent_decode(req.uri().path().as_bytes())
                    .decode_utf8_lossy()
                    .into_owned();

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
                                return short_response_boxed(
                                    StatusCode::NOT_FOUND,
                                    NOT_FOUND_MESSAGE,
                                );
                            }
                            colllection_index = cnum;
                        }
                    }
                    if new_path.is_some() {
                        path = new_path.unwrap();
                    }
                    let base_dir = &get_config().base_dirs[colllection_index];
                    if path.starts_with("/audio/") {
                        debug!(
                            "Received request with following headers {:?}",
                            req.headers()
                        );

                        let range = req
                            .headers()
                            .get(RANGE)
                            .and_then(|h| Range::parse_header(&h.as_ref().into()).ok());
                        let bytes_range = match range {
                            Some(Range::Bytes(bytes_ranges)) => {
                                if bytes_ranges.is_empty() {
                                    return short_response_boxed(
                                        StatusCode::BAD_REQUEST,
                                        "One range is required",
                                    );
                                } else if bytes_ranges.len() > 1 {
                                    return short_response_boxed(
                                        StatusCode::NOT_IMPLEMENTED,
                                        "Do not support muptiple ranges",
                                    );
                                } else {
                                    Some(bytes_ranges[0].clone())
                                }
                            }
                            Some(_) => {
                                return short_response_boxed(
                                    StatusCode::NOT_IMPLEMENTED,
                                    "Other then bytes ranges are not supported",
                                )
                            }
                            None => None,
                        };
                        let seek: Option<f32> = params
                            .as_mut()
                            .and_then(|p| p.remove("seek"))
                            .and_then(|s| s.parse().ok());
                        let transcoding_quality: Option<QualityLevel> = params
                            .and_then(|mut p| p.remove("trans"))
                            .and_then(|t| QualityLevel::from_letter(&t));

                        send_file(
                            base_dir,
                            get_subpath(&path, "/audio/"),
                            bytes_range,
                            seek,
                            transcoding,
                            transcoding_quality,
                        )
                    } else if path.starts_with("/folder/") {
                        get_folder(base_dir, get_subpath(&path, "/folder/"))
                    } else if path == "/search" {
                        if let Some(search_string) = params.and_then(|mut p| p.remove("q")) {
                            return search(base_dir, searcher, search_string.into_owned());
                        }
                        short_response_boxed(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE)
                    } else if path.starts_with("/cover/") {
                        send_file_simple(
                            base_dir,
                            get_subpath(&path, "/cover"),
                            Some(FOLDER_INFO_FILES_CACHE_AGE),
                        )
                    } else if path.starts_with("/desc/") {
                        send_file_simple(
                            base_dir,
                            get_subpath(&path, "/desc"),
                            Some(FOLDER_INFO_FILES_CACHE_AGE),
                        )
                    } else {
                        short_response_boxed(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE)
                    }
                }
            }

            _ => short_response_boxed(StatusCode::METHOD_NOT_ALLOWED, "Method not supported"),
        }
    }
}

#[cfg(feature = "symlinks")]
fn get_real_file_type<P: AsRef<Path>>(
    dir_entry: &DirEntry,
    full_path: P,
    allow_symlinks: bool,
) -> Result<::std::fs::FileType, io::Error> {
    let ft = dir_entry.file_type()?;

    if allow_symlinks && ft.is_symlink() {
        let p = read_link(dir_entry.path())?;
        let ap = if p.is_relative() {
            full_path.as_ref().join(p)
        } else {
            p
        };
        Ok(ap.metadata()?.file_type())
    } else {
        Ok(ft)
    }
}

#[cfg(not(feature = "symlinks"))]
fn get_real_file_type<P: AsRef<Path>>(
    dir_entry: &DirEntry,
    _full_path: P,
    _allow_symlinks: bool,
) -> Result<::std::fs::FileType, io::Error> {
    dir_entry.file_type()
}
