use std::{borrow::Cow, collections::HashMap, fmt::Display, net::IpAddr};

use bytes::{Bytes, BytesMut};
use headers::HeaderMapExt;
use hyper::{body::HttpBody, Body, Request};
use percent_encoding::percent_decode;
use url::form_urlencoded;

use crate::{
    config::{get_config, Cors},
    error,
};

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

pub struct RequestWrapper {
    request: Request<Body>,
    path: String,
    remote_addr: IpAddr,
    #[allow(dead_code)]
    is_ssl: bool,
    #[allow(dead_code)]
    is_behind_proxy: bool,
}

impl RequestWrapper {
    pub fn new(
        request: Request<Body>,
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
            Some(Err(e)) => Err(e),
            None => Ok(Bytes::new()),
        }
    }

    #[allow(dead_code)]
    pub fn into_request(self) -> Request<Body> {
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

    pub fn is_cors_enabled(&self) -> bool {
        RequestWrapper::is_cors_enabled_for_request(&self.request)
    }

    pub fn is_cors_enabled_for_request(req: &Request<Body>) -> bool {
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
}
