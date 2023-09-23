use std::io::Write;

use flate2::{write::GzEncoder, Compression};
use headers::{ContentEncoding, ContentLength};
use http::{response::Builder, Response};
use hyper::Body;

use crate::util::ResponseBuilderExt;

pub fn compressed_response(response_builder: Builder, data: Vec<u8>) -> Response<Body> {
    let output = {
        let mut writer =
            GzEncoder::new(Vec::with_capacity(data.len() / 10), Compression::default());
        writer.write_all(&data).unwrap();
        writer.finish().unwrap()
    };
    let size = output.len() as u64;

    response_builder
        .typed_header(ContentLength(size))
        .typed_header(ContentEncoding::gzip())
        .body(output.into())
        .unwrap()
}
