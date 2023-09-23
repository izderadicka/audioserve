use std::io::Write;

use brotli::CompressorWriter;
use headers::ContentLength;
use http::{response::Builder, Response};
use hyper::Body;

use crate::util::ResponseBuilderExt;
const BROTLI_QUALITY: u32 = 4; // should be goog compromise between speed and size
const BROTLI_LGWIN: u32 = 22; // This is default, don't know it's impact
const BROTLI_BUFFER_SIZE: usize = 8 * 1024;

pub fn compressed_response(response_builder: Builder, data: Vec<u8>) -> Response<Body> {
    let mut output = Vec::with_capacity(data.len() / 10);
    {
        let mut writer = CompressorWriter::new(
            &mut output,
            BROTLI_BUFFER_SIZE,
            BROTLI_QUALITY,
            BROTLI_LGWIN,
        );
        writer.write_all(&data).unwrap();
        writer.flush().unwrap();
    }
    let size = output.len() as u64;

    response_builder
        .typed_header(ContentLength(size))
        .header("Content-Encoding", "br")
        .body(output.into())
        .unwrap()
}
