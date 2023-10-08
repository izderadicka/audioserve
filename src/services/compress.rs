use std::{
    io::{self, Write},
    pin::Pin,
    task::{Context, Poll},
};

use crate::util::ResponseBuilderExt;
use flate2::{write::GzEncoder, Compression};
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

pub struct CompressStream<T> {
    src: Option<T>,
    buf: Vec<u8>,
    filled: usize,
}

impl<T: AsyncRead + Unpin> Stream for CompressStream<T> {
    type Item = Result<Vec<u8>, io::Error>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let myself = self.get_mut();
        if let Some(ref mut src) = myself.src {
            let mut buf = ReadBuf::new(&mut myself.buf[..]);
            buf.set_filled(myself.filled);
            while buf.remaining() > 0 {
                match {
                    let pinned_stream = Pin::new(&mut *src);
                    pinned_stream.poll_read(ctx, &mut buf)
                } {
                    Poll::Ready(Ok(_)) => {
                        let read = buf.filled().len();
                        if read == myself.filled {
                            break;
                        } else {
                            myself.filled = read;
                        }
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                }
            }
            if !buf.filled().is_empty() {
                let chunk = buf.filled().to_vec();
                myself.filled = 0;
                Poll::Ready(Some(Ok(chunk)))
            } else {
                myself.src.take();
                Poll::Ready(None)
            }
        } else {
            error!("Polling after stream is done");
            Poll::Ready(None)
        }
    }
}

impl<T: AsyncRead> CompressStream<T> {
    pub fn new(src: T) -> Self {
        Self::new_with_chunk_size(src, 8 * 1014)
    }
    pub fn new_with_chunk_size(src: T, chunk_size: usize) -> Self {
        CompressStream {
            src: Some(src),
            buf: vec![0u8; chunk_size],
            filled: 0,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use futures::StreamExt;
    use tokio::{fs::File, io::AsyncReadExt};

    #[tokio::test]
    async fn test_stream() -> anyhow::Result<()> {
        let chunk_sizes = &[1, 10, 101, 1024, 10_000, 100_000];
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
        let mut result = String::with_capacity(content.len());
        while let Some(Ok(chunk)) = chunk_stream.next().await {
            assert!(chunk.len() <= chunk_size);
            result.push_str(std::str::from_utf8(&chunk)?);
        }
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
