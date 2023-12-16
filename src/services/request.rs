use std::{borrow::Cow, collections::HashMap, fmt::Display, iter::once, net::IpAddr};

use bytes::Bytes;
use headers::{Header, HeaderMapExt, HeaderName, HeaderValue};
use http_body_util::BodyExt;
use hyper::{
    body::{Body, Incoming},
    Request,
};
use percent_encoding::percent_decode;
use url::form_urlencoded;

use crate::{
    config::{get_config, Cors},
    error,
};

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
        let val = values
            .next()
            .cloned()
            .ok_or_else(|| headers::Error::invalid())?;
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

impl<'a> QueryParams<'a> {
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
    remote_addr: IpAddr,
    #[allow(dead_code)]
    is_ssl: bool,
    #[allow(dead_code)]
    is_behind_proxy: bool,
    can_br_compress: bool,
}

impl<T> GenericRequestWrapper<T>
where
    T: Body + Send + Sync + 'static + Unpin,
{
    pub fn new(
        request: GenericRequest<T>,
        path_prefix: Option<&str>,
        remote_addr: IpAddr,
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
        if segments.any(|s| s.starts_with('.')) {
            return Err(error::Error::msg(
                "Illegal path, contains either special directories or hidden name",
            ));
        }

        let path = match path_prefix {
            Some(p) => match path.strip_prefix(p) {
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
        let can_compress = if get_config().compress_responses {
            match request.headers().typed_get::<AcceptEncoding>() {
                Some(h) => h.accepts("gzip"),
                None => false,
            }
        } else {
            false
        };
        Ok(GenericRequestWrapper {
            request,
            path,
            remote_addr,
            is_ssl,
            is_behind_proxy,
            can_br_compress: can_compress,
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
        Some(RemoteIpAddr::Direct(self.remote_addr))
    }

    pub fn headers(&self) -> &hyper::HeaderMap {
        self.request.headers()
    }

    pub fn method(&self) -> &hyper::Method {
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
            //try scommon  proxy headers
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
        self.can_br_compress
    }

    pub async fn body_bytes(&mut self) -> Result<Bytes, T::Error> {
        let body = self.request.body_mut();
        body.collect().await.map(|collected| collected.to_bytes())
    }

    pub fn is_cors_enabled(&self) -> bool {
        is_cors_enabled_for_request(&self.request)
    }
}

pub fn is_cors_enabled_for_request<B>(req: &GenericRequest<B>) -> bool
where
    B: Body,
{
    if let Some(cors) = get_config().cors.as_ref() {
        match &cors.allow {
            Cors::AllowAllOrigins => true,
            Cors::AllowMatchingOrigins(re) => req
                .headers()
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
                        re.is_match(s)
                    }
                })
                .unwrap_or(false),
        }
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accept_encoding() {
        let header = AcceptEncoding(HeaderValue::from_static("gzip, deflate, br"));
        assert!(header.accepts("br"));
        let header = AcceptEncoding(HeaderValue::from_static("gzip, deflate"));
        assert!(!header.accepts("br"));
    }
}
