use std::io;
use std::task::{Context, Poll};
use std::{future::Future, pin::Pin, time::SystemTime};

use futures::prelude::*;
use headers::{CacheControl, ContentLength, ContentType, LastModified};
use http::response::Builder;
use hyper::{Body, Response, StatusCode};
use tokio::io::{AsyncRead, ReadBuf};

use crate::error::Error;
use crate::util::ResponseBuilderExt;

const NOT_FOUND_MESSAGE: &str = "Not Found";
const TOO_MANY_REQUESTS_MSG: &str = "Too many requests";
const ACCESS_DENIED_MSG: &str = "Access denied";
const METHOD_NOT_ALLOWED_MSG: &str = "Method not supported";
const BAD_REQUEST_MSG: &str = "Bad request";
const NOT_IMPLEMENTED_MSG: &str = "Not Implemented";
const INTERNAL_SERVER_ERROR: &str = "Internal server error";
const UNPROCESSABLE_ENTITY: &str = "Ignored";

pub type ResponseResult = Result<Response<Body>, Error>;
pub type ResponseFuture = Pin<Box<dyn Future<Output = ResponseResult> + Send>>;

fn short_response(status: StatusCode, msg: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .typed_header(ContentLength(msg.len() as u64))
        .typed_header(ContentType::text())
        .body(msg.into())
        .unwrap()
}

pub fn not_found_cached(caching: Option<u32>) -> Response<Body> {
    let mut builder = Response::builder()
        .status(StatusCode::NOT_FOUND)
        .typed_header(ContentLength(NOT_FOUND_MESSAGE.len() as u64))
        .typed_header(ContentType::text());

    builder = add_cache_headers(builder, caching, None);

    builder.body(NOT_FOUND_MESSAGE.into()).unwrap()
}

#[inline]
pub fn fut<F>(f: F) -> ResponseFuture
where
    F: FnOnce() -> Response<Body>,
{
    Box::pin(future::ok(f()))
}

macro_rules! def_resp {
    ($($name:ident ( $code:expr, $msg:expr ));+) => {
        $(
        #[allow(dead_code)]
        pub fn $name() -> Response<Body> {
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
        ChunkStream::new_with_limit(src, std::u64::MAX)
    }
    pub fn new_with_limit(src: T, remains: u64) -> Self {
        ChunkStream {
            src: Some(src),
            remains,
            buf: [0u8; 8 * 1024],
        }
    }
}
