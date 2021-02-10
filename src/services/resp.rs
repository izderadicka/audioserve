use futures::future;
use headers::{ContentLength, ContentType};
use hyper::{Body, Response, StatusCode};

use super::ResponseFuture;
use crate::util::ResponseBuilderExt;

const NOT_FOUND_MESSAGE: &str = "Not Found";
const TOO_MANY_REQUESTS_MSG: &str = "Too many requests";
const ACCESS_DENIED_MSG: &str = "Access denied";
const METHOD_NOT_ALLOWED_MSG: &str = "Method not supported";
const INTERNAL_ERROR_MSG: &str = "Internal Server Error";

pub fn short_response(status: StatusCode, msg: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .typed_header(ContentLength(msg.len() as u64))
        .typed_header(ContentType::text())
        .body(msg.into())
        .unwrap()
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
    internal_error(StatusCode::INTERNAL_SERVER_ERROR,INTERNAL_ERROR_MSG)
);
