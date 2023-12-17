use std::{
    io::{self, Write},
    mem,
    pin::Pin,
    task::{Context, Poll},
    time::SystemTime,
};

use super::ResponseBuilderExt;
use flate2::{write::GzEncoder, Compress, Compression, Crc, FlushCompress};
use futures::Stream;
use headers::{ContentEncoding, ContentLength};
use http::response::Builder;
use tokio::io::{AsyncRead, ReadBuf};

use super::{body::full_body, HttpResponse};

const COMPRESSION_LIMIT: u64 = 512;

#[inline]
pub fn make_sense_to_compress<T: TryInto<u64>>(size: T) -> bool {
    match size.try_into() {
        Ok(size) => size >= COMPRESSION_LIMIT,
        Err(_) => false,
    }
}

pub fn compressed_response(response_builder: Builder, data: Vec<u8>) -> HttpResponse {
    let output = compress_buf(&data);
    let size = output.len() as u64;

    response_builder
        .typed_header(ContentLength(size))
        .typed_header(ContentEncoding::gzip())
        .body(full_body(output))
        .unwrap()
}

pub fn compress_buf(data: &[u8]) -> Vec<u8> {
    let mut writer = GzEncoder::new(Vec::with_capacity(data.len() / 10), Compression::default());
    writer.write_all(&data).expect("Compression error");
    writer.finish().unwrap()
}

#[inline]
fn create_output_buffer(chunk_size: usize) -> Vec<u8> {
    vec![0u8; chunk_size]
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

#[derive(Debug)]
enum State<T> {
    Reading {
        src: T,
        buf_in: Vec<u8>,
        offset_in: usize,
    },
    Dumping,
    Crc {
        crc_bytes: [u8; 8],
        bytes_written: usize,
    },
    Done,
    Processing,
}

pub struct CompressStream<T> {
    state: State<T>,
    buf_out: Vec<u8>,
    offset_out: usize,
    compressor: Compress,
    crc: Crc,
    chunk_size: usize,
}

impl<T> CompressStream<T> {
    fn prepare_output(&mut self) -> Vec<u8> {
        let mut chunk = mem::replace(&mut self.buf_out, create_output_buffer(self.chunk_size));
        chunk.truncate(self.offset_out);
        self.offset_out = 0;
        chunk
    }

    fn compress(&mut self, input: &[u8]) -> io::Result<(usize, usize)> {
        let out_before = self.compressor.total_out();
        let in_before = self.compressor.total_in();
        match self.compressor.compress(
            input,
            &mut self.buf_out[self.offset_out..],
            FlushCompress::None,
        ) {
            Ok(_) => {
                let used = (self.compressor.total_in() - in_before) as usize;
                let produced = (self.compressor.total_out() - out_before) as usize;
                self.crc.update(&input[..used]);
                self.offset_out += produced;
                Ok((used, produced))
            }
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }

    fn dump_compressed(&mut self) -> io::Result<usize> {
        let out_before = self.compressor.total_out();
        match self.compressor.compress(
            &[],
            &mut self.buf_out[self.offset_out..],
            FlushCompress::Finish,
        ) {
            Ok(_) => {
                let produced = (self.compressor.total_out() - out_before) as usize;
                self.offset_out += produced;
                Ok(produced)
            }
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }

    fn crc_footer(&self) -> [u8; 8] {
        let (sum, amt) = (self.crc.sum(), self.crc.amount());
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
}

impl<T: AsyncRead + Unpin> Stream for CompressStream<T> {
    type Item = Result<Vec<u8>, io::Error>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let myself = self.get_mut();
        loop {
            match mem::replace(&mut myself.state, State::Processing) {
                State::Reading {
                    mut src,
                    mut buf_in,
                    mut offset_in,
                } => {
                    let mut buf = ReadBuf::new(&mut buf_in[..]);
                    buf.set_filled(offset_in);
                    match {
                        let pinned_stream = Pin::new(&mut src);
                        pinned_stream.poll_read(ctx, &mut buf)
                    } {
                        Poll::Ready(Ok(_)) => {
                            let read = buf.filled().len();
                            if read == 0 {
                                // no more data on input
                                myself.state = State::Dumping;
                                continue;
                            } else {
                                match myself.compress(buf.filled()) {
                                    Ok((used, _produced)) => {
                                        if used < buf.filled().len() {
                                            let sz = buf.filled().len();
                                            offset_in = sz - used;
                                            let buf_orig = buf.filled_mut();
                                            buf_orig.copy_within(used..sz, 0);
                                        } else {
                                            offset_in = 0;
                                        }

                                        myself.state = State::Reading {
                                            src,
                                            buf_in,
                                            offset_in,
                                        };
                                        if myself.offset_out >= myself.buf_out.len()
                                            || (used == 0 && myself.offset_out > 0)
                                        {
                                            let chunk = myself.prepare_output();
                                            return Poll::Ready(Some(Ok(chunk)));
                                        }
                                    }
                                    Err(e) => {
                                        myself.state = State::Done;
                                        return Poll::Ready(Some(Err(io::Error::new(
                                            io::ErrorKind::Other,
                                            e,
                                        ))));
                                    }
                                }
                            }
                        }
                        Poll::Ready(Err(e)) => {
                            myself.state = State::Done;
                            return Poll::Ready(Some(Err(e)));
                        }
                        Poll::Pending => {
                            myself.state = State::Reading {
                                src,
                                buf_in,
                                offset_in,
                            };
                            return Poll::Pending;
                        }
                    }
                }
                State::Dumping => match myself.dump_compressed() {
                    Ok(produced) => {
                        if produced == 0 {
                            myself.state = State::Crc {
                                crc_bytes: myself.crc_footer(),
                                bytes_written: 0,
                            }
                        } else {
                            myself.state = State::Dumping;
                            if myself.offset_out >= myself.buf_out.len() {
                                let chunk = myself.prepare_output();
                                return Poll::Ready(Some(Ok(chunk)));
                            }
                        }
                    }
                    Err(e) => {
                        myself.state = State::Done;
                        return Poll::Ready(Some(Err(e)));
                    }
                },
                State::Crc {
                    crc_bytes,
                    mut bytes_written,
                } => {
                    let left = crc_bytes.len() - bytes_written;
                    if left == 0 {
                        myself.state = State::Done;
                    } else {
                        let space = myself.buf_out.len() - myself.offset_out;
                        let can_write = space.min(left);
                        (&mut myself.buf_out[myself.offset_out..myself.offset_out + can_write])
                            .copy_from_slice(&crc_bytes[bytes_written..bytes_written + can_write]);
                        bytes_written += can_write;
                        myself.offset_out += can_write;
                        let chunk = myself.prepare_output();
                        myself.state = State::Crc {
                            crc_bytes,
                            bytes_written,
                        };
                        return Poll::Ready(Some(Ok(chunk)));
                    }
                }
                State::Done => {
                    myself.state = State::Done;
                    return Poll::Ready(None);
                }
                State::Processing => {
                    unreachable!("Should not get here - temporary state of stream")
                }
            }
        }
    }
}

impl<T: AsyncRead> CompressStream<T> {
    pub fn new(src: T) -> Self {
        Self::new_with_chunk_size(src, 8 * 1024)
    }
    pub fn new_with_chunk_size(src: T, chunk_size: usize) -> Self {
        assert!(chunk_size >= 10);
        let header = gzip_header(Compression::default());
        let mut buf_out = create_output_buffer(chunk_size);
        (&mut buf_out[0..header.len()]).copy_from_slice(&header);
        let offset_out = header.len();

        let state = State::Reading {
            src,
            buf_in: vec![0u8; chunk_size],
            offset_in: 0,
        };

        CompressStream {
            state,
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
    use ring::rand::{SecureRandom, SystemRandom};
    use tokio::{fs::File, io::AsyncReadExt};

    #[tokio::test]
    async fn test_stream() -> anyhow::Result<()> {
        let chunk_sizes = &[10, 101, 1024, 10_000, 100_000];
        for chunk_size in chunk_sizes {
            test_stream_with_chunk_size(*chunk_size).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_big_random_data() -> anyhow::Result<()> {
        let rng = SystemRandom::new();
        let mut data = vec![0u8; 10_000_000];
        rng.fill(&mut data).unwrap();
        let mut chunk_stream = CompressStream::new_with_chunk_size(&data[..], 16 * 1024);
        let mut compressed: Vec<u8> = Vec::with_capacity(data.len());
        while let Some(Ok(chunk)) = chunk_stream.next().await {
            compressed.extend(&chunk);
        }
        let buf: Vec<u8> = Vec::with_capacity(data.len());
        let uncompressed = {
            let mut decoder = GzDecoder::new(buf);
            decoder.write_all(&compressed)?;
            decoder.finish()?
        };

        assert_eq!(data.len(), uncompressed.len(), "Size differs");
        assert_eq!(data, uncompressed, "Content differs");

        Ok(())
    }

    async fn test_stream_with_chunk_size(chunk_size: usize) -> anyhow::Result<()> {
        let file_name = "src/response/compress.rs";
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
            assert!(
                chunk.len() <= chunk_size,
                "chunk len {}, chunk_size {}",
                chunk.len(),
                chunk_size
            );
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
