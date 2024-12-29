use std::{borrow::Cow, collections::HashMap, fmt::Display, iter::once, net::IpAddr};

use bytes::Bytes;
use headers::{Header, HeaderMapExt, HeaderName, HeaderValue};
use http::Request;
use http_body_util::BodyExt;
use hyper::body::{Body, Incoming};
use percent_encoding::percent_decode;
use regex::Regex;
use url::form_urlencoded;

use crate::error;

pub struct AcceptEncoding(HeaderValue);

type GenericRequest<T> = Request<T>;
pub type HttpRequest = GenericRequest<Incoming>;
pub type RequestWrapper = GenericRequestWrapper<Incoming>;

impl Header for AcceptEncoding {
    fn name() -> &'static HeaderName {
        &http::header::ACCEPT_ENCODING
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        Self: Sized,
        I: Iterator<Item = &'i HeaderValue>,
    {
        let val = values.next().cloned().ok_or_else(headers::Error::invalid)?;
        Ok(AcceptEncoding(val))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        values.extend(once(self.0.clone()))
    }
}

impl AcceptEncoding {
    pub fn accepts(&self, encoding: &str) -> bool {
        self.0
            .to_str()
            .ok()
            .and_then(|s| {
                s.split(',')
                    .find(|token| token.trim().to_ascii_lowercase() == encoding)
            })
            .map(|_| true)
            .unwrap_or(false)
    }
}

pub struct QueryParams<'a> {
    params: Option<HashMap<Cow<'a, str>, Cow<'a, str>>>,
}

impl QueryParams<'_> {
    pub fn get<S: AsRef<str>>(&self, name: S) -> Option<&Cow<'_, str>> {
        self.params.as_ref().and_then(|m| m.get(name.as_ref()))
    }

    pub fn exists<S: AsRef<str>>(&self, name: S) -> bool {
        self.get(name).is_some()
    }

    pub fn get_string<S: AsRef<str>>(&self, name: S) -> Option<String> {
        self.get(name).map(|s| s.to_string())
    }
}

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

pub struct GenericRequestWrapper<T> {
    request: GenericRequest<T>,
    path: String,
    remote_addr: Option<IpAddr>,
    #[allow(dead_code)]
    is_ssl: bool,
    #[allow(dead_code)]
    is_behind_proxy: bool,
    can_compress: bool,
    is_cors: bool,
}

//Builder pattern for options
impl<T> GenericRequestWrapper<T> {
    pub fn set_remote_addr(mut self, remote_addr: Option<IpAddr>) -> Self {
        self.remote_addr = remote_addr;
        self
    }
    pub fn set_is_ssl(mut self, is_ssl: bool) -> Self {
        self.is_ssl = is_ssl;
        self
    }
    pub fn set_is_behind_proxy(mut self, is_behind_proxy: bool) -> Self {
        self.is_behind_proxy = is_behind_proxy;
        self
    }
    pub fn set_can_compress(mut self, can_compress: bool) -> Self {
        self.can_compress = if can_compress {
            match self.request.headers().typed_get::<AcceptEncoding>() {
                Some(h) => h.accepts("gzip"),
                None => false,
            }
        } else {
            false
        };
        self
    }
    pub fn set_is_cors(mut self, is_cors: bool) -> Self {
        self.is_cors = is_cors;
        self
    }

    pub fn set_path_prefix(mut self, path_prefix: Option<&str>) -> error::Result<Self> {
        self.path = match path_prefix {
            Some(p) => self
                .path
                .strip_prefix(p)
                .map(|p| p.to_string())
                .ok_or_else(|| {
                    error!("URL path is missing prefix {}", p);
                    error::Error::msg(format!("URL path is missing prefix {}", p))
                })?,
            None => self.path,
        };
        Ok(self)
    }
}

impl<T> GenericRequestWrapper<T>
where
    T: Body + Send + Sync + 'static + Unpin,
{
    pub fn new(request: GenericRequest<T>) -> error::Result<Self> {
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
        if segments.any(|s| s.starts_with('.')) {
            return Err(error::Error::msg(
                "Illegal path, contains either special directories or hidden name",
            ));
        }

        Ok(GenericRequestWrapper {
            request,
            path,
            remote_addr: None,
            is_ssl: false,
            is_behind_proxy: false,
            can_compress: false,
            is_cors: false,
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
                .and_then(|fwd| fwd.client().copied())
                .map(RemoteIpAddr::Proxied)
                .or_else(|| {
                    self.request
                        .headers()
                        .typed_get::<proxy_headers::XForwardedFor>()
                        .map(|xfwd| RemoteIpAddr::Proxied(*xfwd.client()))
                });
        }
        self.remote_addr.map(RemoteIpAddr::Direct)
    }

    pub fn headers(&self) -> &http::HeaderMap {
        self.request.headers()
    }

    pub fn method(&self) -> &http::Method {
        self.request.method()
    }

    #[allow(dead_code)]
    pub fn into_body(self) -> T {
        self.request.into_body()
    }

    #[allow(dead_code)]
    pub fn into_request(self) -> GenericRequest<T> {
        self.request
    }

    pub fn params(&self) -> QueryParams<'_> {
        QueryParams {
            params: self
                .request
                .uri()
                .query()
                .map(|query| form_urlencoded::parse(query.as_bytes()).collect::<HashMap<_, _>>()),
        }
    }

    pub fn is_https(&self) -> bool {
        if self.is_ssl {
            return true;
        }
        #[cfg(feature = "behind-proxy")]
        if self.is_behind_proxy {
            //try common  proxy headers
            let forwarded_https = self
                .request
                .headers()
                .typed_get::<proxy_headers::Forwarded>()
                .and_then(|fwd| fwd.client_protocol().map(|p| p.as_ref() == "https"))
                .unwrap_or(false);

            if forwarded_https {
                return true;
            }

            return self
                .request
                .headers()
                .get("X-Forwarded-Proto")
                .map(|v| v.as_bytes() == b"https")
                .unwrap_or(false);
        }
        false
    }

    pub fn can_compress(&self) -> bool {
        self.can_compress
    }

    pub async fn body_bytes(&mut self) -> Result<Bytes, T::Error> {
        let body = self.request.body_mut();
        body.collect().await.map(|collected| collected.to_bytes())
    }

    pub fn is_cors_enabled(&self) -> bool {
        self.is_cors
    }
}

pub fn is_cors_matching_origin<B>(req: &GenericRequest<B>, matching_regex: &Regex) -> bool
where
    B: Body,
{
    req.headers()
        .get("origin")
        .and_then(|v| {
            v.to_str()
                .map_err(|e| error!("Invalid origin header: {}", e))
                .ok()
        })
        .map(|s| {
            if s.to_ascii_lowercase() == "null" {
                false
            } else {
                matching_regex.is_match(s)
            }
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use crate::response::body::empty_body;

    use super::*;

    #[test]
    fn test_accept_encoding() {
        let header = AcceptEncoding(HeaderValue::from_static("gzip, deflate, br"));
        assert!(header.accepts("br"));
        let header = AcceptEncoding(HeaderValue::from_static("gzip, deflate"));
        assert!(!header.accepts("br"));
    }

    // Returns true when the origin header matches the provided regex
    #[test]
    fn returns_true_when_origin_matches_regex() {
        use http::Request;
        use regex::Regex;

        let req = Request::builder()
            .header("origin", "https://example.com")
            .body(empty_body())
            .unwrap();

        let regex = Regex::new(r"https://example\.\w{2,5}").unwrap();
        assert!(is_cors_matching_origin(&req, &regex));
    }

    #[test]
    fn test_request_wrapper() {
        let req = Request::builder()
            .uri("https://example.com?a=1&b=2")
            .body(empty_body())
            .unwrap();
        let req = GenericRequestWrapper::new(req).unwrap();
        let req = req.set_is_cors(true).set_is_ssl(true);

        assert!(req.is_cors_enabled());
        assert!(req.is_https());

        let params = req.params();
        assert_eq!(params.get("a"), Some(&Cow::Borrowed("1")));
        assert_eq!(params.get("b"), Some(&Cow::Borrowed("2")));
    }
}
