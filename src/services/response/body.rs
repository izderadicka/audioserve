use std::convert::Infallible;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full, StreamBody};
use hyper::body::Frame;

pub type HttpBody = BoxBody<Bytes, Infallible>;

pub fn full_body<T: Into<Bytes>>(bytes: T) -> HttpBody {
    Full::new(bytes.into()).boxed()
}

pub fn empty_body() -> HttpBody {
    Empty::new().boxed()
}

pub fn wrap_stream<S>(stream: S) -> HttpBody
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + Sync + 'static,
{
    let body = StreamBody::new(stream.map(|b| Ok::<_, Infallible>(Frame::data(b.unwrap()))));
    BodyExt::boxed(body)
}
