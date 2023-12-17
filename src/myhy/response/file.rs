use collection::guess_mime_type;
use headers::{AcceptRanges, ContentEncoding, ContentLength, ContentRange, ContentType};
use http::{header::CONTENT_ENCODING, Response, StatusCode};
use std::{
    cmp::{max, min},
    ffi::{OsStr, OsString},
    io::{self, SeekFrom},
    ops::{Bound, RangeBounds},
    path::{Path, PathBuf},
};
use tokio::{fs, io::AsyncSeekExt};

use super::{
    add_cache_headers,
    body::wrap_stream,
    compress::{make_sense_to_compress, CompressStream},
    not_found, ChunkStream, HttpResponse, ResponseBuilderExt, ResponseResult,
};
use crate::error::Error;

pub type ByteRange = (Bound<u64>, Bound<u64>);

pub async fn serve_opened_file(
    mut file: tokio::fs::File,
    range: Option<ByteRange>,
    caching: Option<u32>,
    mime: mime::Mime,
) -> Result<HttpResponse, io::Error> {
    let meta = file.metadata().await?;
    let file_len = meta.len();
    if file_len == 0 {
        warn!("File has zero size ")
    }
    let last_modified = meta.modified().ok();
    let mut resp = Response::builder().typed_header(ContentType::from(mime));
    resp = add_cache_headers(resp, caching, last_modified);

    let full_range = || (0, file_len.saturating_sub(1));

    let (start, end) = match range {
        Some(range) => match to_satisfiable_range(range, file_len) {
            Some(l) => {
                resp = resp.status(StatusCode::PARTIAL_CONTENT).typed_header(
                    ContentRange::bytes(into_range_bounds(l), Some(file_len)).unwrap(),
                );
                l
            }
            None => {
                error!("Wrong range {:?}", range);
                full_range()
            }
        },
        None => {
            resp = resp
                .status(StatusCode::OK)
                .typed_header(AcceptRanges::bytes());
            full_range()
        }
    };
    let _pos = file.seek(SeekFrom::Start(start)).await;
    let sz = end - start + 1;
    let stream = ChunkStream::new_with_limit(file, sz);
    let resp = resp
        .typed_header(ContentLength(sz))
        .body(wrap_stream(stream))
        .unwrap();
    Ok(resp)
}

fn to_satisfiable_range<T: RangeBounds<u64>>(r: T, len: u64) -> Option<(u64, u64)> {
    match (r.start_bound(), r.end_bound()) {
        (Bound::Included(&start), Bound::Included(&end)) => {
            if start <= end && start < len {
                Some((start, min(end, len - 1)))
            } else {
                None
            }
        }

        (Bound::Included(&start), Bound::Unbounded) => {
            if start < len {
                Some((start, len - 1))
            } else {
                None
            }
        }

        (Bound::Unbounded, Bound::Included(&offset)) => {
            if offset > 0 {
                Some((max(len - offset, 0), len - 1))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn into_range_bounds(i: (u64, u64)) -> (Bound<u64>, Bound<u64>) {
    (Bound::Included(i.0), Bound::Included(i.1))
}

pub async fn serve_file_from_fs(
    full_path: &Path,
    range: Option<ByteRange>,
    caching: Option<u32>,
    compressed: bool,
) -> ResponseResult {
    let filename: PathBuf = full_path.into();
    match fs::File::open(&filename).await {
        Ok(file) => {
            let mime = guess_mime_type(&filename);
            if compressed {
                serve_compressed_file(file, caching, mime).await
            } else {
                serve_opened_file(file, range, caching, mime).await
            }
            .map_err(Error::new)
        }
        Err(e) => {
            error!("Error when sending file {:?} : {}", filename, e);
            Ok(not_found())
        }
    }
}

async fn serve_compressed_file(
    file: tokio::fs::File,
    caching: Option<u32>,
    mime: mime::Mime,
) -> Result<HttpResponse, io::Error> {
    let meta = file.metadata().await?;
    let last_modified = meta.modified().ok();
    let file_size = meta.len();

    let mut resp = Response::builder().typed_header(ContentType::from(mime));
    resp = add_cache_headers(resp, caching, last_modified);
    resp = resp.status(StatusCode::OK);

    let body = if make_sense_to_compress(file_size) {
        resp = resp.typed_header(ContentEncoding::gzip());
        let stream = CompressStream::new(file);
        wrap_stream(stream)
    } else {
        resp = resp.typed_header(ContentLength(file_size));
        let stream = ChunkStream::new(file);
        wrap_stream(stream)
    };
    let resp = resp.body(body).unwrap();

    Ok(resp)
}

pub async fn send_file_simple<P: AsRef<Path>>(
    base_path: &'static Path,
    file_path: P,
    cache: Option<u32>,
    compressed: bool,
) -> ResponseResult {
    let full_path = base_path.join(&file_path);
    serve_file_from_fs(&full_path, None, cache, compressed).await
}

pub async fn send_static_file<P: AsRef<Path> + Send>(
    base_path: &'static Path,
    file_path: P,
    cache: Option<u32>,
) -> ResponseResult {
    let full_path = base_path.join(&file_path);
    fn append_ext(ext: impl AsRef<OsStr>, path: &PathBuf) -> PathBuf {
        let mut os_string: OsString = path.into();
        os_string.push(ext.as_ref());
        os_string.into()
    }

    let compressed_name = append_ext(".gz", &full_path);
    if let Ok(true) = fs::try_exists(&compressed_name).await {
        let mime = guess_mime_type(&full_path);
        let file = fs::File::open(compressed_name).await?;
        let mut resp = serve_opened_file(file, None, cache, mime).await?;
        resp.headers_mut()
            .insert(CONTENT_ENCODING, "gzip".parse().unwrap());
        Ok(resp)
    } else {
        serve_file_from_fs(&full_path, None, cache, false).await
    }
}
