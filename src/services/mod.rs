use self::auth::{AuthResult, Authenticator};
use self::search::Search;
use self::subs::{
    collections_list, get_folder, recent, search, send_file, send_file_simple, transcodings_list,
    ResponseFuture,
};
use self::transcode::QualityLevel;
use crate::config::get_config;
use crate::util::ResponseBuilderExt;
use crate::{error, util::header2header};
use bytes::{Bytes, BytesMut};
use collection::{Collections, FoldersOrdering};
use futures::prelude::*;
use futures::{future, TryFutureExt};
use headers::{
    AccessControlAllowCredentials, AccessControlAllowHeaders, AccessControlAllowMethods,
    AccessControlAllowOrigin, AccessControlMaxAge, AccessControlRequestHeaders, HeaderMapExt,
    Origin, Range,
};
use hyper::StatusCode;
use hyper::{body::HttpBody, service::Service, Body, Method, Request, Response};
use leaky_cauldron::Leaky;
use percent_encoding::percent_decode;
use regex::Regex;
use std::iter::FromIterator;
use std::time::Duration;
use std::{
    borrow::Cow,
    collections::HashMap,
    convert::Infallible,
    fmt::Display,
    net::IpAddr,
    net::SocketAddr,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc},
    task::Poll,
};
use url::form_urlencoded;

pub mod auth;
#[cfg(feature = "shared-positions")]
pub mod position;
pub mod resp;
pub mod search;
mod subs;
pub mod transcode;
mod types;

const APP_STATIC_FILES_CACHE_AGE: u32 = 30 * 24 * 3600;
const FOLDER_INFO_FILES_CACHE_AGE: u32 = 24 * 3600;

type Counter = Arc<AtomicUsize>;

#[derive(Debug)]
pub enum RemoteIpAddr {
    Direct(IpAddr),
    #[allow(dead_code)]
    Proxied(IpAddr),
}

impl AsRef<IpAddr> for RemoteIpAddr {
    fn as_ref(&self) -> &IpAddr {
        match self {
            RemoteIpAddr::Direct(a) => a,
            RemoteIpAddr::Proxied(a) => a,
        }
    }
}

impl Display for RemoteIpAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteIpAddr::Direct(a) => a.fmt(f),
            RemoteIpAddr::Proxied(a) => write!(f, "Proxied: {}", a),
        }
    }
}

pub struct RequestWrapper {
    request: Request<Body>,
    path: String,
    remote_addr: Option<IpAddr>,
    #[allow(dead_code)]
    is_ssl: bool,
    #[allow(dead_code)]
    is_behind_proxy: bool,
}

impl RequestWrapper {
    pub fn new(
        request: Request<Body>,
        path_prefix: Option<&str>,
        remote_addr: Option<IpAddr>,
        is_ssl: bool,
    ) -> error::Result<Self> {
        let path = match percent_decode(request.uri().path().as_bytes()).decode_utf8() {
            Ok(s) => s.into_owned(),
            Err(e) => {
                return Err(error::Error::msg(format!(
                    "Invalid path encoding, not UTF-8: {}",
                    e
                )))
            }
        };
        //Check for unwanted path segments - e.g. ., .., .anything - so we do not want special directories and hidden directories and files
        let mut segments = path.split('/');
        if segments.any(|s| s.starts_with(".")) {
            return Err(error::Error::msg(
                "Illegal path, contains either special directories or hidden name",
            ));
        }

        let path = match path_prefix {
            Some(p) => match crate::util::strip_prefix_of(p, &path) {
                //TODO: later replace with new std function strip_prefix
                Some(s) => {
                    if s.is_empty() {
                        "/".to_string()
                    } else {
                        s.to_string()
                    }
                }
                None => {
                    error!("URL path is missing prefix {}", p);
                    return Err(error::Error::msg(format!(
                        "URL path is missing prefix {}",
                        p
                    )));
                }
            },
            None => path,
        };
        let is_behind_proxy = get_config().behind_proxy;
        Ok(RequestWrapper {
            request,
            path,
            remote_addr,
            is_ssl,
            is_behind_proxy,
        })
    }

    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    pub fn remote_addr(&self) -> Option<RemoteIpAddr> {
        #[cfg(feature = "behind-proxy")]
        if self.is_behind_proxy {
            return self
                .request
                .headers()
                .typed_get::<proxy_headers::Forwarded>()
                .and_then(|fwd| fwd.client().map(|c| c.clone()))
                .map(|addr| RemoteIpAddr::Proxied(addr))
                .or_else(|| {
                    self.request
                        .headers()
                        .typed_get::<proxy_headers::XForwardedFor>()
                        .map(|xfwd| RemoteIpAddr::Proxied(*xfwd.client()))
                });
        }
        self.remote_addr.map(RemoteIpAddr::Direct)
    }

    pub fn headers(&self) -> &hyper::HeaderMap {
        self.request.headers()
    }

    pub fn method(&self) -> &hyper::Method {
        self.request.method()
    }

    #[allow(dead_code)]
    pub fn into_body(self) -> Body {
        self.request.into_body()
    }

    pub async fn body_bytes(&mut self) -> Result<Bytes, hyper::Error> {
        let first = self.request.body_mut().data().await;
        match first {
            Some(Ok(data)) => {
                let mut buf = BytesMut::from(&data[..]);
                while let Some(res) = self.request.body_mut().data().await {
                    let next = res?;
                    buf.extend_from_slice(&next);
                }
                Ok(buf.into())
            }
            Some(Err(e)) => return Err(e),
            None => Ok(Bytes::new()),
        }
    }

    #[allow(dead_code)]
    pub fn into_request(self) -> Request<Body> {
        self.request
    }

    pub fn params(&self) -> Option<HashMap<Cow<str>, Cow<str>>> {
        self.request
            .uri()
            .query()
            .map(|query| form_urlencoded::parse(query.as_bytes()).collect::<HashMap<_, _>>())
    }

    pub fn is_https(&self) -> bool {
        if self.is_ssl {
            return true
        }
        #[cfg(feature = "behind-proxy")]
        if self.is_behind_proxy {
            //try some known proxy headers
            // let forwarded_https self
            //     .request
            //     .headers()
            //     .typed_get::<proxy_headers::Forwarded>()
            //     .and_then(|fwd| )

            return self.request.headers()
            .get("X-Forwarded-Proto").map(|v| v.as_bytes() == b"https").unwrap_or(false);

        }
        false
    }
}
#[derive(Clone)]
pub struct TranscodingDetails {
    pub transcodings: Counter,
    pub max_transcodings: usize,
}

pub struct ServiceFactory<T> {
    authenticator: Option<Arc<Box<dyn Authenticator<Credentials = T>>>>,
    rate_limitter: Option<Arc<Leaky>>,
    search: Search<String>,
    transcoding: TranscodingDetails,
    collections: Arc<Collections>,
}

impl<T> ServiceFactory<T> {
    pub fn new<A>(
        auth: Option<A>,
        search: Search<String>,
        transcoding: TranscodingDetails,
        collections: Arc<Collections>,
        rate_limit: Option<f32>,
    ) -> Self
    where
        A: Authenticator<Credentials = T> + 'static,
    {
        ServiceFactory {
            authenticator: auth
                .map(|a| Arc::new(Box::new(a) as Box<dyn Authenticator<Credentials = T>>)),
            rate_limitter: rate_limit.map(|l| Arc::new(Leaky::new(l))),
            search,
            transcoding,
            collections,
        }
    }

    pub fn create(
        &self,
        remote_addr: Option<SocketAddr>,
        is_ssl: bool,
    ) -> impl Future<Output = Result<FileSendService<T>, Infallible>> {
        future::ok(FileSendService {
            authenticator: self.authenticator.clone(),
            rate_limitter: self.rate_limitter.clone(),
            search: self.search.clone(),
            transcoding: self.transcoding.clone(),
            collections: self.collections.clone(),
            remote_addr,
            is_ssl,
        })
    }
}

#[derive(Clone)]
pub struct FileSendService<T> {
    pub authenticator: Option<Arc<Box<dyn Authenticator<Credentials = T>>>>,
    pub rate_limitter: Option<Arc<Leaky>>,
    pub search: Search<String>,
    pub transcoding: TranscodingDetails,
    pub collections: Arc<Collections>,
    pub remote_addr: Option<SocketAddr>,
    pub is_ssl: bool,
}

// use only on checked prefixes
fn get_subpath(path: &str, prefix: &str) -> PathBuf {
    Path::new(&path).strip_prefix(prefix).unwrap().to_path_buf()
}

fn add_cors_headers(
    mut resp: Response<Body>,
    origin: Option<Origin>,
    enabled: bool,
) -> Response<Body> {
    if !enabled {
        return resp;
    }
    match origin {
        Some(o) => {
            if let Ok(allowed_origin) = header2header::<_, AccessControlAllowOrigin>(o) {
                let headers = resp.headers_mut();
                headers.typed_insert(allowed_origin);
                headers.typed_insert(AccessControlAllowCredentials);
            }
            resp
        }
        None => resp,
    }
}

fn preflight_cors_response(req: &Request<Body>) -> Response<Body> {
    let origin = req.headers().typed_get::<Origin>();
    const ALLOWED_METHODS: &[Method] = &[Method::GET, Method::POST, Method::OPTIONS];

    let mut resp_builder = Response::builder()
        .status(StatusCode::NO_CONTENT)
        // Allow all requested headers
        .typed_header(AccessControlAllowMethods::from_iter(
            ALLOWED_METHODS.iter().cloned(),
        ))
        .typed_header(AccessControlMaxAge::from(Duration::from_secs(24 * 3600)));

    if let Some(requested_headers) = req.headers().typed_get::<AccessControlRequestHeaders>() {
        resp_builder = resp_builder.typed_header(AccessControlAllowHeaders::from_iter(
            requested_headers.iter(),
        ));
    }

    let resp = resp_builder.body(Body::empty()).unwrap();

    add_cors_headers(resp, origin, true)
}

#[allow(clippy::type_complexity)]
impl<C: 'static> Service<Request<Body>> for FileSendService<C> {
    type Response = Response<Body>;
    type Error = error::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        //Limit rate of requests in configured
        if let Some(limiter) = self.rate_limitter.as_ref() {
            if let Err(_) = limiter.start_one() {
                debug!("Rejecting request due to rate limit");
                return resp::fut(resp::too_many_requests);
            }
        }

        // handle OPTIONS method for CORS preflightAtomicUsize
        if req.method() == Method::OPTIONS && get_config().cors {
            debug!(
                "Got OPTIONS request in CORS mode : {} {:?}",
                req.uri(),
                req.headers()
            );
            return Box::pin(future::ok(preflight_cors_response(&req)));
        }

        let req = match RequestWrapper::new(
            req,
            get_config().url_path_prefix.as_deref(),
            self.remote_addr.map(|a| a.ip()),
            self.is_ssl,
        ) {
            Ok(r) => r,
            Err(e) => {
                error!("Request URL error: {}", e);
                return resp::fut(resp::not_found);
            }
        };
        //static files
        if req.method() == Method::GET {
            if req.path() == "/" {
                return send_file_simple(
                    &get_config().client_dir,
                    "index.html",
                    Some(APP_STATIC_FILES_CACHE_AGE),
                );
            };
            if req.path() == "/bundle.js" {
                return send_file_simple(
                    &get_config().client_dir,
                    "bundle.js",
                    Some(APP_STATIC_FILES_CACHE_AGE),
                );
            }
        }
        // from here everything must be authenticated
        let searcher = self.search.clone();
        let transcoding = self.transcoding.clone();
        let cors = get_config().cors;
        let origin = req.headers().typed_get::<Origin>();

        let resp = match self.authenticator {
            Some(ref auth) => {
                let collections = self.collections.clone();
                Box::pin(auth.authenticate(req).and_then(move |result| match result {
                    AuthResult::Authenticated { request, .. } => {
                        FileSendService::<C>::process_checked(
                            request,
                            searcher,
                            transcoding,
                            collections,
                        )
                    }
                    AuthResult::LoggedIn(resp) | AuthResult::Rejected(resp) => {
                        Box::pin(future::ok(resp))
                    }
                }))
            }
            None => FileSendService::<C>::process_checked(
                req,
                searcher,
                transcoding,
                self.collections.clone(),
            ),
        };
        Box::pin(resp.map_ok(move |r| add_cors_headers(r, origin, cors)))
    }
}

impl<C> FileSendService<C> {
    fn process_checked(
        req: RequestWrapper,
        searcher: Search<String>,
        transcoding: TranscodingDetails,
        collections: Arc<Collections>,
    ) -> ResponseFuture {
        let params = req.params();

        match *req.method() {
            Method::GET => {
                let path = req.path();

                if path.starts_with("/collections") {
                    collections_list()
                } else if path.starts_with("/transcodings") {
                    transcodings_list()
                } else if cfg!(feature = "shared-positions") && path.starts_with("/position") {
                    #[cfg(not(feature = "shared-positions"))]
                    unimplemented!();
                    #[cfg(feature = "shared-positions")]
                    self::position::position_service(req, collections)
                } else {
                    let (path, colllection_index) = match extract_collection_number(path) {
                        Ok(r) => r,
                        Err(_) => {
                            error!("Invalid collection number");
                            return resp::fut(resp::not_found);
                        }
                    };

                    let base_dir = &get_config().base_dirs[colllection_index];
                    let ord = params
                        .as_ref()
                        .and_then(|p| p.get("ord").map(|l| FoldersOrdering::from_letter(l)))
                        .unwrap_or(FoldersOrdering::Alphabetical);
                    if path.starts_with("/audio/") {
                        FileSendService::<C>::serve_audio(
                            &req,
                            base_dir,
                            &path,
                            transcoding,
                            params,
                        )
                    } else if path.starts_with("/folder/") {
                        get_folder(
                            colllection_index,
                            get_subpath(&path, "/folder/"),
                            collections,
                            ord,
                        )
                    } else if !get_config().disable_folder_download && path.starts_with("/download")
                    {
                        #[cfg(feature = "folder-download")]
                        {
                            let format = params
                                .as_ref()
                                .and_then(|p| p.get("fmt"))
                                .and_then(|f| f.parse::<types::DownloadFormat>().ok())
                                .unwrap_or_default();
                            subs::download_folder(
                                base_dir,
                                get_subpath(&path, "/download/"),
                                format,
                            )
                        }
                        #[cfg(not(feature = "folder-download"))]
                        {
                            error!("folder download not ");
                            resp::fut(resp::not_found)
                        }
                    } else if path == "/search" {
                        if let Some(search_string) = params.and_then(|mut p| p.remove("q")) {
                            search(colllection_index, searcher, search_string.into_owned(), ord)
                        } else {
                            error!("q parameter is missing in search");
                            resp::fut(resp::not_found)
                        }
                    } else if path.starts_with("/recent") {
                        recent(colllection_index, searcher)
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
                        error!("Invalid path requested {}", path);
                        resp::fut(resp::not_found)
                    }
                }
            }

            _ => resp::fut(resp::method_not_supported),
        }
    }

    fn serve_audio(
        req: &RequestWrapper,
        base_dir: &'static Path,
        path: &str,
        transcoding: TranscodingDetails,
        mut params: Option<HashMap<std::borrow::Cow<str>, std::borrow::Cow<str>>>,
    ) -> ResponseFuture {
        debug!(
            "Received request with following headers {:?}",
            req.headers()
        );

        let range = req.headers().typed_get::<Range>();

        let bytes_range = match range.map(|r| r.iter().collect::<Vec<_>>()) {
            Some(bytes_ranges) => {
                if bytes_ranges.is_empty() {
                    error!("Range header without range bytes");
                    return resp::fut(resp::bad_request);
                } else if bytes_ranges.len() > 1 {
                    error!("Range with multiple ranges is not supported");
                    return resp::fut(resp::not_implemented);
                } else {
                    Some(bytes_ranges[0])
                }
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
    }
}

lazy_static! {
    static ref COLLECTION_NUMBER_RE: Regex = Regex::new(r"^/(\d+)/.+").unwrap();
}

fn extract_collection_number(path: &str) -> Result<(&str, usize), ()> {
    let matches = COLLECTION_NUMBER_RE.captures(&path);
    if let Some(matches) = matches {
        let cnum = matches.get(1).unwrap();
        // match gives us char position it's safe to slice
        let new_path = &path[cnum.end()..];
        // and cnum is guarateed to contain digits only
        let cnum: usize = cnum.as_str().parse().unwrap();
        if cnum >= get_config().base_dirs.len() {
            return Err(());
        }
        Ok((new_path, cnum))
    } else {
        Ok((path, 0))
    }
}
