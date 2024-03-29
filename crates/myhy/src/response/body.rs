use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full, StreamBody};
use hyper::body::Frame;

pub type HttpBody = BoxBody<Bytes, std::io::Error>;

pub fn full_body<T: Into<Bytes>>(bytes: T) -> HttpBody {
    Full::new(bytes.into())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, ""))
        .boxed()
}

pub fn empty_body() -> HttpBody {
    Empty::new()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, ""))
        .boxed()
}

pub fn wrap_stream<S, T>(stream: S) -> HttpBody
where
    T: Into<Bytes>,
    S: Stream<Item = Result<T, std::io::Error>> + Send + Sync + 'static,
{
    let body = StreamBody::new(stream.map(|res| res.map(|data| Frame::data(data.into()))));
    BodyExt::boxed(body)
}
