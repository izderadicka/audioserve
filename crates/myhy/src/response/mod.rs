use std::convert::Infallible;
use std::io;
use std::str::FromStr;
use std::task::{Context, Poll};
use std::{future::Future, pin::Pin, time::SystemTime};

use body::empty_body;
use bytes::Bytes;
use futures::prelude::*;
use headers::{
    CacheControl, ContentEncoding, ContentLength, ContentType, Header, HeaderMapExt, LastModified,
};
use http::response::Builder;
use http::{header, Response, StatusCode};
use http_body_util::BodyExt;
use hyper::body::Body;
use mime::Mime;
use tokio::io::{AsyncRead, ReadBuf};

use self::body::{full_body, HttpBody};
use self::compress::{compress_buf, compressed_response, make_sense_to_compress};
use crate::error::Error;

pub mod body;
pub mod compress;
pub mod cors;
pub mod file;

const NOT_FOUND_MESSAGE: &str = "Not Found";
const TOO_MANY_REQUESTS_MSG: &str = "Too many requests";
const ACCESS_DENIED_MSG: &str = "Access denied";
const METHOD_NOT_ALLOWED_MSG: &str = "Method not supported";
const BAD_REQUEST_MSG: &str = "Bad request";
const NOT_IMPLEMENTED_MSG: &str = "Not Implemented";
const INTERNAL_SERVER_ERROR: &str = "Internal server error";
const UNPROCESSABLE_ENTITY: &str = "Ignored";

pub type HttpResponse = Response<HttpBody>;
pub type ResponseResult = Result<HttpResponse, Error>;
pub type ResponseFuture = Pin<Box<dyn Future<Output = ResponseResult> + Send>>;

pub trait ResponseBuilderExt {
    fn typed_header<H: Header>(self, header: H) -> Self;
}

impl ResponseBuilderExt for Builder {
    fn typed_header<H: Header>(mut self, header: H) -> Builder {
        if let Some(h) = self.headers_mut() {
            h.typed_insert(header)
        };
        self
    }
}

pub fn box_websocket_response<B>(response: Response<B>) -> HttpResponse
where
    B: Body<Data = Bytes, Error = Infallible> + Send + Sync + 'static,
{
    let (parts, body) = response.into_parts();
    let body = BodyExt::boxed(body.map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "")));
    Response::from_parts(parts, body)
}

fn short_response(status: StatusCode, msg: &'static str) -> HttpResponse {
    Response::builder()
        .status(status)
        .typed_header(ContentLength(msg.len() as u64))
        .typed_header(ContentType::text())
        .body(full_body(msg))
        .unwrap()
}

pub fn not_found_cached(caching: Option<u32>) -> HttpResponse {
    let mut builder = Response::builder()
        .status(StatusCode::NOT_FOUND)
        .typed_header(ContentLength(NOT_FOUND_MESSAGE.len() as u64))
        .typed_header(ContentType::text());

    builder = add_cache_headers(builder, caching, None);

    builder.body(full_body(NOT_FOUND_MESSAGE)).unwrap()
}

#[inline]
pub fn fut<F>(f: F) -> ResponseFuture
where
    F: FnOnce() -> HttpResponse,
{
    Box::pin(future::ok(f()))
}

macro_rules! def_resp {
    ($($name:ident ( $code:expr, $msg:expr ));+) => {
        $(
        #[allow(dead_code)]
        pub fn $name() -> HttpResponse {
            short_response($code, $msg)
        }
    )+
    }
}

def_resp!(
    deny(StatusCode::UNAUTHORIZED, ACCESS_DENIED_MSG);
    too_many_requests(StatusCode::TOO_MANY_REQUESTS, TOO_MANY_REQUESTS_MSG);
    not_found(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE);
    method_not_supported(StatusCode::METHOD_NOT_ALLOWED, METHOD_NOT_ALLOWED_MSG);
    bad_request(StatusCode::BAD_REQUEST, BAD_REQUEST_MSG);
    not_implemented(StatusCode::NOT_IMPLEMENTED, NOT_IMPLEMENTED_MSG);
    created(StatusCode::CREATED, "");
    internal_error(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_SERVER_ERROR);
    ignored(StatusCode::UNPROCESSABLE_ENTITY, UNPROCESSABLE_ENTITY)
);

pub fn redirect_permanent(url: &str) -> HttpResponse {
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(header::LOCATION, url)
        .body(empty_body())
        .unwrap()
}

pub fn add_cache_headers(
    mut resp: Builder,
    caching: Option<u32>,
    last_modified: Option<SystemTime>,
) -> Builder {
    if let Some(age) = caching {
        if age > 0 {
            let cache = CacheControl::new()
                .with_public()
                .with_max_age(std::time::Duration::from_secs(u64::from(age)));
            resp = resp.typed_header(cache);
        }
        if let Some(last_modified) = last_modified {
            resp = resp.typed_header(LastModified::from(last_modified));
        }
    } else {
        resp = resp.typed_header(CacheControl::new().with_no_store());
    }

    resp
}

pub fn data_response<T>(
    data: T,
    content_type: Mime,
    cache_age: Option<u32>,
    last_modified: Option<SystemTime>,
    should_compress: bool,
) -> HttpResponse
where
    T: Into<Bytes>,
{
    let mut buf: Bytes = data.into();
    let mut resp = Response::builder().typed_header(ContentType::from(content_type));
    if should_compress && make_sense_to_compress(buf.len()) {
        buf = compress_buf(&buf).into();
        resp = resp.typed_header(ContentEncoding::gzip());
    }
    resp = resp
        .typed_header(ContentLength(buf.len() as u64))
        .status(StatusCode::OK);
    resp = add_cache_headers(resp, cache_age, last_modified);

    resp.body(full_body(buf)).map_err(Error::from).unwrap()
}

pub fn json_response<T: serde::Serialize>(data: &T, compress: bool) -> HttpResponse {
    let json = serde_json::to_string(data).expect("Serialization error");

    let builder = Response::builder().typed_header(ContentType::json());
    if compress && make_sense_to_compress(json.len()) {
        compressed_response(builder, json.into_bytes())
    } else {
        builder
            .typed_header(ContentLength(json.len() as u64))
            .body(full_body(json))
            .unwrap()
    }
}

pub fn xml_response<T: Into<String>>(
    data: T,
    compress: bool,
    specific_mime: Option<&str>,
) -> HttpResponse {
    let xml: String = data.into();
    let mime = specific_mime
        .and_then(|m| Mime::from_str(m).ok())
        .unwrap_or(mime::TEXT_XML);
    let builder = Response::builder().typed_header(ContentType::from(mime));
    if compress && make_sense_to_compress(xml.len()) {
        compressed_response(builder, xml.into_bytes())
    } else {
        builder
            .typed_header(ContentLength(xml.len() as u64))
            .body(full_body(xml))
            .unwrap()
    }
}

pub struct ChunkStream<T> {
    src: Option<T>,
    remains: u64,
    buf: [u8; 8 * 1024],
}

impl<T: AsyncRead + Unpin> Stream for ChunkStream<T> {
    type Item = Result<Vec<u8>, io::Error>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let pin = self.get_mut();
        if let Some(ref mut src) = pin.src {
            if pin.remains == 0 {
                pin.src.take();
                return Poll::Ready(None);
            }
            let mut buf = ReadBuf::new(&mut pin.buf[..]);
            match futures::ready! {
                {
                let pinned_stream = Pin::new(src);
                pinned_stream.poll_read(ctx, &mut buf)
                }
            } {
                Ok(_) => {
                    let read = buf.filled().len();
                    if read == 0 {
                        pin.src.take();
                        Poll::Ready(None)
                    } else {
                        let to_send = pin.remains.min(read as u64);
                        pin.remains -= to_send;
                        let chunk = pin.buf[..to_send as usize].to_vec();
                        Poll::Ready(Some(Ok(chunk)))
                    }
                }
                Err(e) => Poll::Ready(Some(Err(e))),
            }
        } else {
            error!("Polling after stream is done");
            Poll::Ready(None)
        }
    }
}

impl<T: AsyncRead> ChunkStream<T> {
    pub fn new(src: T) -> Self {
        ChunkStream::new_with_limit(src, u64::MAX)
    }
    pub fn new_with_limit(src: T, remains: u64) -> Self {
        ChunkStream {
            src: Some(src),
            remains,
            buf: [0u8; 8 * 1024],
        }
    }
}
