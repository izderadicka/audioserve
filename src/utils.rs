use hyper;
use hyper::server::Response;
use hyper:: {StatusCode};
use hyper::header::ContentLength;
use futures::future::{self, Future};


pub type ResponseFuture = Box<Future<Item = Response, Error = hyper::Error>>;

pub fn short_response(status: StatusCode, msg: &'static str) -> Response {
    Response::new()
    .with_status(status)
    .with_header(ContentLength(msg.len() as u64))
    .with_body(msg)

}

pub fn short_response_boxed(status: StatusCode, msg: &'static str) -> ResponseFuture {
    Box::new(future::ok(short_response(status, msg)))
}

