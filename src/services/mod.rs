use self::auth::{AuthResult, Authenticator};
use self::request::{HttpRequest, QueryParams, RequestWrapper};
use self::response::{HttpResponse, ResponseFuture, ResponseResult};
use self::search::Search;
use self::transcode::QualityLevel;
use crate::config::get_config;
use crate::services::response::body::empty_body;
use crate::services::transcode::ChosenTranscoding;
use crate::util::ResponseBuilderExt;
use crate::{error, util::header2header};

use collection::{Collections, FoldersOrdering};
use futures::prelude::*;
use futures::{future, TryFutureExt};
use headers::{
    AccessControlAllowCredentials, AccessControlAllowHeaders, AccessControlAllowMethods,
    AccessControlAllowOrigin, AccessControlMaxAge, AccessControlRequestHeaders, HeaderMapExt,
    Origin, Range, UserAgent,
};
use hyper::StatusCode;
use hyper::{service::Service, Method, Request, Response};
use leaky_cauldron::Leaky;

use regex::Regex;
use std::iter::FromIterator;
use std::time::Duration;
use std::{
    convert::Infallible,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{atomic::AtomicUsize, Arc},
    task::Poll,
};

pub mod api;
pub mod auth;
pub mod compress;
mod files;
pub mod icon;
#[cfg(feature = "shared-positions")]
pub mod position;
pub mod request;
pub mod response;
pub mod search;
pub mod transcode;
mod types;

type Counter = Arc<AtomicUsize>;

#[derive(Clone)]
pub struct TranscodingDetails {
    pub transcodings: Counter,
    pub max_transcodings: usize,
}

pub struct ServiceFactory<T> {
    authenticator: Option<Arc<dyn Authenticator<Credentials = T>>>,
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
            authenticator: auth.map(|a| Arc::new(a) as Arc<dyn Authenticator<Credentials = T>>),
            rate_limitter: rate_limit.map(|l| Arc::new(Leaky::new(l))),
            search,
            transcoding,
            collections,
        }
    }

    pub fn create(
        &self,
        remote_addr: SocketAddr,
        is_ssl: bool,
    ) -> impl Future<Output = Result<MainService<T>, Infallible>> {
        future::ok(MainService {
            state: ServiceComponents {
                search: self.search.clone(),
                transcoding: self.transcoding.clone(),
                collections: self.collections.clone(),
            },
            authenticator: self.authenticator.clone(),
            rate_limitter: self.rate_limitter.clone(),
            remote_addr,
            is_ssl,
        })
    }
}

#[derive(Clone)]
pub struct ServiceComponents {
    pub search: Search<String>,
    pub transcoding: TranscodingDetails,
    pub collections: Arc<Collections>,
}

type OptionalAuthenticatorType<T> = Option<Arc<dyn Authenticator<Credentials = T>>>;

#[derive(Clone)]
pub struct MainService<T> {
    pub state: ServiceComponents,
    pub authenticator: OptionalAuthenticatorType<T>,
    pub rate_limitter: Option<Arc<Leaky>>,
    pub remote_addr: SocketAddr,
    pub is_ssl: bool,
}

// use only on checked prefixes
fn get_subpath(path: &str, prefix: &str) -> PathBuf {
    Path::new(&path).strip_prefix(prefix).unwrap().to_path_buf()
}

fn add_cors_headers(mut resp: HttpResponse, origin: Option<Origin>, enabled: bool) -> HttpResponse {
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

fn preflight_cors_response(req: &HttpRequest) -> HttpResponse {
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

    let resp = resp_builder.body(empty_body()).unwrap();

    add_cors_headers(resp, origin, true)
}

const STATIC_FILE_NAMES: &[&str] = &[
    "/bundle.js",
    "/bundle.css",
    "/global.css",
    "/favicon.png",
    "/app.webmanifest",
    "/service-worker.js",
];

const STATIC_DIR: &str = "/static/";

fn is_static_file(path: &str) -> bool {
    STATIC_FILE_NAMES.contains(&path) || path.starts_with(STATIC_DIR)
}

#[allow(clippy::type_complexity)]
impl<C: Send + 'static> Service<HttpRequest> for MainService<C> {
    type Response = HttpResponse;
    type Error = error::Error;
    type Future = ResponseFuture;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HttpRequest) -> Self::Future {
        let state = self.state.clone();

        //Limit rate of requests if configured
        if let Some(ref limiter) = self.rate_limitter {
            if limiter.start_one().is_err() {
                debug!("Rejecting request due to rate limit");
                return response::fut(response::too_many_requests);
            }
        }

        // handle OPTIONS method for CORS preflight
        if req.method() == Method::OPTIONS && RequestWrapper::is_cors_enabled_for_request(&req) {
            debug!(
                "Got OPTIONS request in CORS mode : {} {:?}",
                req.uri(),
                req.headers()
            );
            return response::fut(|| preflight_cors_response(&req));
        }

        let req = match RequestWrapper::new(
            req,
            get_config().url_path_prefix.as_deref(),
            self.remote_addr.ip(),
            self.is_ssl,
        ) {
            Ok(r) => r,
            Err(e) => {
                error!("Request URL error: {}", e);
                return response::fut(response::bad_request);
            }
        };

        Box::pin(
            MainService::<C>::process_request(state, self.authenticator.clone(), req).or_else(
                |e| {
                    error!("Request processing error: {}", e);
                    future::ok(response::internal_error())
                },
            ),
        )
    }
}

impl<C: Send + 'static> MainService<C> {
    async fn process_request(
        subservices: ServiceComponents,
        authenticator: OptionalAuthenticatorType<C>,
        req: RequestWrapper,
    ) -> ResponseResult {
        //static files
        if req.method() == Method::GET {
            if req.path() == "/" || req.path() == "/index.html" {
                return files::send_static_file(
                    &get_config().client_dir,
                    "index.html",
                    get_config().static_resource_cache_age,
                )
                .await;
            } else if is_static_file(req.path()) {
                return files::send_static_file(
                    &get_config().client_dir,
                    &req.path()[1..],
                    get_config().static_resource_cache_age,
                )
                .await;
            }
        }
        // from here everything must be authenticated
        let cors = req.is_cors_enabled();
        let origin = req.headers().typed_get::<Origin>();

        let resp = match authenticator {
            Some(ref auth) => {
                let auth_result = auth.authenticate(req).await;

                match auth_result {
                    Ok(AuthResult::Authenticated { request, .. }) => {
                        MainService::<C>::process_authenticated(request, subservices).await
                    }
                    Ok(AuthResult::LoggedIn(resp)) | Ok(AuthResult::Rejected(resp)) => Ok(resp),
                    Err(e) => Err(e),
                }
            }
            None => MainService::<C>::process_authenticated(req, subservices).await,
        };
        resp.map(move |r| add_cors_headers(r, origin, cors))
    }

    async fn process_authenticated(
        mut req: RequestWrapper,
        subservices: ServiceComponents,
    ) -> ResponseResult {
        let params = req.params();
        let path = req.path();
        let ServiceComponents {
            search,
            transcoding,
            collections,
        } = subservices;
        match *req.method() {
            Method::GET => {
                if path.starts_with("/collections") {
                    api::collections_list(req.can_compress())
                } else if path.starts_with("/transcodings") {
                    let user_agent = req.headers().typed_get::<UserAgent>();
                    api::transcodings_list(
                        user_agent.as_ref().map(|h| h.as_str()),
                        req.can_compress(),
                    )
                } else if cfg!(feature = "shared-positions") && path.starts_with("/positions") {
                    // positions API
                    #[cfg(feature = "shared-positions")]
                    match extract_group(path) {
                        PositionGroup::Group(group) => match position_params(&params) {
                            Ok(p) => {
                                api::all_positions(collections, group, Some(p), req.can_compress())
                                    .await
                            }

                            Err(e) => {
                                error!("Invalid timestamp param: {}", e);
                                response::fut(response::bad_request).await
                            }
                        },
                        PositionGroup::Last(group) => {
                            api::last_position(collections, group, req.can_compress()).await
                        }
                        PositionGroup::Path {
                            collection,
                            group,
                            path,
                        } => {
                            let recursive = req.params().exists("rec");
                            let filter = match position_params(&params) {
                                Ok(p) => p,

                                Err(e) => {
                                    error!("Invalid timestamp param: {}", e);
                                    return Ok(response::bad_request());
                                }
                            };
                            api::folder_position(
                                collections,
                                group,
                                collection,
                                path,
                                recursive,
                                Some(filter),
                                req.can_compress(),
                            )
                            .await
                        }
                        PositionGroup::Malformed => Ok(response::bad_request()),
                    }
                    #[cfg(not(feature = "shared-positions"))]
                    unimplemented!();
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
                            return Ok(response::not_found());
                        }
                    };

                    let base_dir = &get_config().base_dirs[colllection_index];
                    let ord = params
                        .get("ord")
                        .map(|l| FoldersOrdering::from_letter(l))
                        .unwrap_or(FoldersOrdering::Alphabetical);
                    if path.starts_with("/audio/") {
                        MainService::<C>::serve_audio(&req, base_dir, path, transcoding).await
                    } else if path.starts_with("/folder/") {
                        let group = params.get_string("group");
                        api::get_folder(
                            colllection_index,
                            get_subpath(path, "/folder/"),
                            collections,
                            ord,
                            group,
                            req.can_compress(),
                        )
                        .await
                    } else if !get_config().disable_folder_download && path.starts_with("/download")
                    {
                        #[cfg(feature = "folder-download")]
                        {
                            let format = params
                                .get("fmt")
                                .and_then(|f| f.parse::<types::DownloadFormat>().ok())
                                .unwrap_or_default();
                            let recursive = params
                                .get("collapsed")
                                .and_then(|_| get_config().collapse_cd_folders.as_ref())
                                .and_then(|c| c.regex.as_ref())
                                .and_then(|re| Regex::new(re).ok());
                            files::download_folder(
                                base_dir,
                                get_subpath(path, "/download/"),
                                format,
                                recursive,
                            )
                            .await
                        }
                        #[cfg(not(feature = "folder-download"))]
                        {
                            error!("folder download not ");
                            response::fut(response::not_found)
                        }
                    } else if path == "/search" {
                        if let Some(search_string) = params.get_string("q") {
                            let group = params.get_string("group");
                            api::search(
                                colllection_index,
                                search,
                                search_string,
                                ord,
                                group,
                                req.can_compress(),
                            )
                            .await
                        } else {
                            error!("q parameter is missing in search");
                            Ok(response::bad_request())
                        }
                    } else if path.starts_with("/recent") {
                        let group = params.get_string("group");
                        api::recent(colllection_index, search, group, req.can_compress()).await
                    } else if path.starts_with("/cover/") {
                        files::send_cover(
                            base_dir,
                            get_subpath(path, "/cover"),
                            get_config().folder_file_cache_age,
                        )
                        .await
                    } else if path.starts_with("/icon/") {
                        files::send_folder_icon(
                            colllection_index,
                            get_subpath(path, "/icon/"),
                            collections,
                        )
                        .await
                    } else if path.starts_with("/desc/") {
                        files::send_description(
                            base_dir,
                            get_subpath(path, "/desc"),
                            get_config().folder_file_cache_age,
                            req.can_compress(),
                        )
                        .await
                    } else {
                        error!("Invalid path requested {}", path);
                        Ok(response::not_found())
                    }
                }
            }

            Method::POST => {
                #[cfg(feature = "shared-positions")]
                if path.starts_with("/positions") {
                    match extract_group(path) {
                        PositionGroup::Group(group) => {
                            let is_json = req
                                .headers()
                                .get("Content-Type")
                                .and_then(|v| {
                                    v.to_str()
                                        .ok()
                                        .map(|s| s.to_lowercase().eq("application/json"))
                                })
                                .unwrap_or(false);
                            if is_json {
                                match req.body_bytes().await {
                                    Ok(bytes) => {
                                        api::insert_position(collections, group, bytes).await
                                    }
                                    Err(e) => {
                                        error!("Error reading POST body: {}", e);
                                        Ok(response::bad_request())
                                    }
                                }
                            } else {
                                error!("Not JSON content type");
                                Ok(response::bad_request())
                            }
                        }
                        _ => Ok(response::bad_request()),
                    }
                } else {
                    Ok(response::not_found())
                }

                #[cfg(not(feature = "shared-positions"))]
                response::fut(response::method_not_supported)
            }

            _ => Ok(response::method_not_supported()),
        }
    }

    async fn serve_audio(
        req: &RequestWrapper,
        base_dir: &'static Path,
        path: &str,
        transcoding: TranscodingDetails,
    ) -> ResponseResult {
        let params = req.params();
        let user_agent = req.headers().typed_get::<UserAgent>();
        let user_agent = user_agent.as_ref().map(|ua| ua.as_str());
        debug!(
            "Received request with following headers {:?}",
            req.headers()
        );

        let range = req.headers().typed_get::<Range>();

        let bytes_range = match range.map(|r| r.iter().collect::<Vec<_>>()) {
            Some(bytes_ranges) => {
                if bytes_ranges.is_empty() {
                    error!("Range header without range bytes");
                    return Ok(response::bad_request());
                } else if bytes_ranges.len() > 1 {
                    error!("Range with multiple ranges is not supported");
                    return Ok(response::not_implemented());
                } else {
                    Some(bytes_ranges[0])
                }
            }

            None => None,
        };
        let seek: Option<f32> = params.get("seek").and_then(|s| s.parse().ok());
        let transcoding_quality: Option<ChosenTranscoding> = params
            .get("trans")
            .and_then(|t| QualityLevel::from_letter(&t))
            .map(|level| ChosenTranscoding::for_level_and_user_agent(level, user_agent));

        files::send_file(
            base_dir,
            get_subpath(path, "/audio/"),
            bytes_range,
            seek,
            transcoding,
            transcoding_quality,
        )
        .await
    }
}

lazy_static! {
    static ref COLLECTION_NUMBER_RE: Regex = Regex::new(r"^/(\d+)/.+").unwrap();
}

fn extract_collection_number(path: &str) -> Result<(&str, usize), ()> {
    let matches = COLLECTION_NUMBER_RE.captures(path);
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

#[cfg(feature = "shared-positions")]
#[derive(Debug)]
enum PositionGroup {
    Group(String),
    Last(String),
    Path {
        group: String,
        collection: usize,
        path: String,
    },
    Malformed,
}

#[cfg(feature = "shared-positions")]
fn position_params(params: &QueryParams) -> error::Result<collection::PositionFilter> {
    use collection::{audio_meta::TimeStamp, PositionFilter};

    fn get_ts_param(params: &QueryParams, name: &str) -> Result<Option<TimeStamp>, anyhow::Error> {
        Ok(if let Some(ts) = params.get(name) {
            Some(ts.parse::<u64>().map_err(error::Error::new)?).map(TimeStamp::from)
        } else {
            None
        })
    }

    let finished = params.exists("finished");
    let unfinished = params.exists("unfinished");
    let finished = match (finished, unfinished) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    };

    let from = get_ts_param(params, "from")?;
    let to = get_ts_param(params, "to")?;

    Ok(PositionFilter::new(finished, from, to))
}

#[cfg(feature = "shared-positions")]
fn extract_group(path: &str) -> PositionGroup {
    let mut segments = path.splitn(5, '/');
    segments.next(); // read out first empty segment
    segments.next(); // readout positions segment
    if let Some(group) = segments.next().map(|g| g.to_owned()) {
        if let Some(last) = segments.next() {
            if last == "last" {
                //only last position
                return PositionGroup::Last(group);
            } else if let Ok(collection) = last.parse::<usize>() {
                if let Some(path) = segments.next() {
                    return PositionGroup::Path {
                        group,
                        collection,
                        path: path.into(),
                    };
                }
            }
        } else {
            return PositionGroup::Group(group);
        }
    }
    PositionGroup::Malformed
}

#[cfg(test)]
#[cfg(feature = "shared-positions")]
mod tests {
    use super::*;

    #[test]
    fn test_extract_group() {
        if let PositionGroup::Group(x) = extract_group("/positions/usak") {
            assert_eq!(x, "usak");
        } else {
            panic!("group does not match")
        }

        if let PositionGroup::Last(x) = extract_group("/positions/usak/last") {
            assert_eq!(x, "usak");
        } else {
            panic!("group does not match")
        }

        if let PositionGroup::Path {
            path,
            collection,
            group,
        } = extract_group("/positions/usak/0/hrabe/drakula")
        {
            assert_eq!(group, "usak");
            assert_eq!(collection, 0);
            assert_eq!(path, "hrabe/drakula");
        } else {
            panic!("group does not match")
        }

        if let PositionGroup::Malformed = extract_group("/positions/chcip/pes") {
        } else {
            panic!("should be invalid")
        }
    }
}
