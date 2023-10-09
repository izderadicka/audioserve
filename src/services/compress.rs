use std::{
    io::{self, Write},
    mem,
    pin::Pin,
    task::{Context, Poll},
    time::SystemTime,
};

use crate::util::ResponseBuilderExt;
use flate2::{write::GzEncoder, Compress, Compression, Crc, FlushCompress, Status};
use futures::Stream;
use headers::{ContentEncoding, ContentLength};
use http::{response::Builder, Response};
use hyper::Body;
use tokio::io::{AsyncRead, ReadBuf};

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

#[inline]
fn create_output_buffer(chunk_size: usize) -> Vec<u8> {
    vec![0u8; chunk_size + 64]
}

fn gzip_header(lvl: Compression) -> [u8; 10] {
    let mut header = [0u8; 10];
    let mtime: u32 = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;
    header[0] = 0x1f;
    header[1] = 0x8b;
    header[2] = 8;
    header[3] = 0;
    header[4] = (mtime >> 0) as u8;
    header[5] = (mtime >> 8) as u8;
    header[6] = (mtime >> 16) as u8;
    header[7] = (mtime >> 24) as u8;
    header[8] = if lvl.level() >= Compression::best().level() {
        2
    } else if lvl.level() <= Compression::fast().level() {
        4
    } else {
        0
    };

    // Typically this byte indicates what OS the gz stream was created on,
    // but in an effort to have cross-platform reproducible streams just
    // default this value to 255. I'm not sure that if we "correctly" set
    // this it'd do anything anyway...
    header[9] = 255;
    header
}

fn crc_footer(crc: &Crc) -> [u8; 8] {
    let (sum, amt) = (crc.sum(), crc.amount());
    let buf = [
        (sum >> 0) as u8,
        (sum >> 8) as u8,
        (sum >> 16) as u8,
        (sum >> 24) as u8,
        (amt >> 0) as u8,
        (amt >> 8) as u8,
        (amt >> 16) as u8,
        (amt >> 24) as u8,
    ];
    buf
}

pub struct CompressStream<T> {
    src: Option<T>,
    buf_in: Vec<u8>,
    buf_out: Vec<u8>,
    offset_out: usize,
    compressor: Compress,
    crc: Crc,
    chunk_size: usize,
}

impl<T> CompressStream<T> {}

impl<T: AsyncRead + Unpin> Stream for CompressStream<T> {
    type Item = Result<Vec<u8>, io::Error>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let myself = self.get_mut();
        if let Some(ref mut src) = myself.src {
            let mut input_finished = false;
            while myself.offset_out < myself.buf_out.len() - 8 {
                // should keep there place for CRC
                let mut buf = ReadBuf::new(&mut myself.buf_in[..]);
                match {
                    let pinned_stream = Pin::new(&mut *src);
                    pinned_stream.poll_read(ctx, &mut buf)
                } {
                    Poll::Ready(Ok(_)) => {
                        let read = buf.filled().len();
                        if read == 0 {
                            // no more data on input
                            input_finished = true;
                            break;
                        } else {
                            let out_before = myself.compressor.total_out();
                            let in_before = myself.compressor.total_in();
                            match myself.compressor.compress(
                                buf.filled(),
                                &mut myself.buf_out[myself.offset_out..],
                                FlushCompress::None,
                            ) {
                                Ok(_) => {
                                    let used = (myself.compressor.total_in() - in_before) as usize;
                                    let produced =
                                        (myself.compressor.total_out() - out_before) as usize;

                                    myself.crc.update(&buf.filled()[..used]);
                                    myself.offset_out += produced;

                                    if used < buf.filled().len() {
                                        //TODO
                                        todo!("we need to return unused bytes to begining")
                                    }

                                    if produced == 0 {
                                        // No data outputted yet
                                        continue;
                                    }
                                }
                                Err(e) => {
                                    return Poll::Ready(Some(Err(io::Error::new(
                                        io::ErrorKind::Other,
                                        e,
                                    ))))
                                }
                            }
                        }
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                }
            }
            if input_finished {
                myself.src.take();
                // write rest of data to output
                let before_out = myself.compressor.total_out();
                match myself.compressor.compress(
                    &[],
                    &mut myself.buf_out[myself.offset_out..],
                    FlushCompress::Finish,
                ) {
                    Ok(Status::BufError) => todo!("Need to extend buf_out"),
                    Ok(_) => {
                        let produced = (myself.compressor.total_out() - before_out) as usize;
                        myself.offset_out += produced;
                        let crc = crc_footer(&myself.crc);
                        let ofs = myself.offset_out;
                        let end = ofs + crc.len();
                        let sz = myself.buf_out.len();
                        if end > sz {
                            myself.buf_out.extend_from_within(sz - (end - sz)..);
                        }
                        (&mut myself.buf_out[ofs..ofs + crc.len()]).clone_from_slice(&crc);
                        myself.offset_out += crc.len();
                    }
                    Err(e) => {
                        return Poll::Ready(Some(Err(io::Error::new(io::ErrorKind::Other, e))))
                    }
                }
            }
            if myself.offset_out > 0 {
                let mut chunk =
                    mem::replace(&mut myself.buf_out, create_output_buffer(myself.chunk_size));
                chunk.truncate(myself.offset_out);
                myself.offset_out = 0;
                Poll::Ready(Some(Ok(chunk)))
            } else {
                Poll::Ready(None)
            }
        } else {
            Poll::Ready(None)
        }
    }
}

impl<T: AsyncRead> CompressStream<T> {
    pub fn new(src: T) -> Self {
        Self::new_with_chunk_size(src, 8 * 1024)
    }
    pub fn new_with_chunk_size(src: T, chunk_size: usize) -> Self {
        let header = gzip_header(Compression::default());
        let mut buf_out = create_output_buffer(chunk_size);
        (&mut buf_out[0..header.len()]).copy_from_slice(&header);
        let offset_out = header.len();

        CompressStream {
            src: Some(src),
            buf_in: vec![0u8; chunk_size],
            buf_out,
            offset_out,
            compressor: Compress::new(Compression::default(), false),
            crc: Crc::new(),
            chunk_size,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use flate2::write::GzDecoder;
    use futures::StreamExt;
    use tokio::{fs::File, io::AsyncReadExt};

    #[tokio::test]
    async fn test_stream() -> anyhow::Result<()> {
        let chunk_sizes = &[101, 1024, 10_000, 100_000];
        for chunk_size in chunk_sizes {
            test_stream_with_chunk_size(*chunk_size).await?;
        }
        Ok(())
    }

    async fn test_stream_with_chunk_size(chunk_size: usize) -> anyhow::Result<()> {
        let file_name = "src/main.rs";
        let mut content = String::new();
        {
            let mut f = File::open(file_name).await?;
            let bytes_read = f.read_to_string(&mut content).await?;
            assert!(bytes_read > 100);
        }
        let f = File::open(file_name).await?;
        let mut chunk_stream = CompressStream::new_with_chunk_size(f, chunk_size);
        let mut compressed: Vec<u8> = Vec::with_capacity(content.len());
        while let Some(Ok(chunk)) = chunk_stream.next().await {
            assert!(chunk.len() <= chunk_size + 1024);
            compressed.extend(&chunk);
        }
        let buf: Vec<u8> = Vec::with_capacity(content.len());
        let uncompressed = {
            let mut decoder = GzDecoder::new(buf);
            decoder.write_all(&compressed)?;
            decoder.finish()?
        };

        let result = String::from_utf8(uncompressed)?;

        assert_eq!(
            content.len(),
            result.len(),
            "Test result for chunk size {}",
            chunk_size
        );
        assert_eq!(content, result);
        Ok(())
    }
}
