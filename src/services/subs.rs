use super::audio_folder::list_dir;
#[cfg(feature = "folder-download")]
use super::audio_folder::list_dir_files_only;
use super::audio_folder::parse_chapter_path;
use super::search::{Search, SearchTrait};
use super::transcode::{guess_format, AudioFilePath, QualityLevel, TimeSpan};
use super::types::*;
use super::Counter;
use crate::config::get_config;
use crate::error::{Error, Result};
use crate::util::{
    checked_dec, guess_mime_type, into_range_bounds, to_satisfiable_range, ResponseBuilderExt,
};
use futures::prelude::*;
use futures::{future, ready, Stream};
use headers::{AcceptRanges, CacheControl, ContentLength, ContentRange, ContentType, LastModified};
#[cfg(feature = "folder-download")]
use hyper::header::CONTENT_DISPOSITION;
use hyper::{Body, Response as HyperResponse, StatusCode};
use std::collections::Bound;
use std::ffi::OsStr;
use std::io::{self, SeekFrom};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncSeekExt, ReadBuf};
use tokio::task::spawn_blocking as blocking;

pub type ByteRange = (Bound<u64>, Bound<u64>);

pub const NOT_FOUND_MESSAGE: &str = "Not Found";
const SEVER_ERROR_TRANSCODING: &str = "Server error during transcoding process";

type Response = HyperResponse<Body>;

pub type ResponseFuture = Pin<Box<dyn Future<Output = Result<Response, Error>> + Send>>;

pub fn short_response(status: StatusCode, msg: &'static str) -> Response {
    HyperResponse::builder()
        .status(status)
        .typed_header(ContentLength(msg.len() as u64))
        .typed_header(ContentType::text())
        .body(msg.into())
        .unwrap()
}

pub fn short_response_boxed(status: StatusCode, msg: &'static str) -> ResponseFuture {
    Box::pin(future::ok(short_response(status, msg)))
}

#[cfg(not(feature = "transcoding-cache"))]
fn serve_file_cached_or_transcoded(
    full_path: PathBuf,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    _range: Option<ByteRange>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: QualityLevel,
) -> ResponseFuture {
    serve_file_transcoded_checked(
        AudioFilePath::Original(full_path),
        seek,
        span,
        transcoding,
        transcoding_quality,
    )
}

#[cfg(feature = "transcoding-cache")]
fn serve_file_cached_or_transcoded(
    full_path: PathBuf,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    range: Option<ByteRange>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: QualityLevel,
) -> ResponseFuture {
    if get_config().transcoding.cache.disabled {
        return serve_file_transcoded_checked(
            AudioFilePath::Original(full_path),
            seek,
            span,
            transcoding,
            transcoding_quality,
        );
    }

    use super::transcode::cache::{cache_key, get_cache};
    let cache = get_cache();
    let cache_key = cache_key(&full_path, transcoding_quality, span);
    let fut = cache
        .get2(cache_key)
        .then(|res| match res {
            Err(e) => {
                error!("Cache lookup error: {}", e);
                future::ok(None)
            }
            Ok(f) => future::ok(f),
        })
        .and_then(move |maybe_file| match maybe_file {
            None => serve_file_transcoded_checked(
                AudioFilePath::Original(full_path),
                seek,
                span,
                transcoding,
                transcoding_quality,
            ),
            Some((f, path)) => {
                if seek.is_some() {
                    debug!(
                        "File is in cache and seek is needed -  will send remuxed from {:?} {:?}",
                        path, span
                    );
                    serve_file_transcoded_checked(
                        AudioFilePath::Transcoded(path),
                        seek,
                        None,
                        transcoding,
                        transcoding_quality,
                    )
                } else {
                    debug!("Sending file {:?} from transcoded cache", &full_path);
                    let mime = get_config()
                        .transcoder(transcoding_quality)
                        .transcoded_mime();
                    Box::pin(serve_opened_file(f, range, None, mime).map_err(|e| {
                        error!("Error sending cached file: {}", e);
                        Error::new(e).context("sending cached file")
                    }))
                }
            }
        });

    Box::pin(fut)
}

fn serve_file_transcoded_checked(
    full_path: AudioFilePath<PathBuf>,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: QualityLevel,
) -> ResponseFuture {
    let counter = transcoding.transcodings;

    // TODO: This is not correct - atomic load and increase should be done together as check and replace
    // however it does not matter much - basic limitation is achieve though not exact
    // to fix we can increment here too , but then need to assure that decrement is also on fail paths
    let running_transcodings: u32 = counter.load(Ordering::SeqCst) as u32;
    if running_transcodings >= transcoding.max_transcodings {
        warn!("Max transcodings reached {}", transcoding.max_transcodings);
        short_response_boxed(StatusCode::SERVICE_UNAVAILABLE, "Max transcodings reached")
    } else {
        debug!(
            "Sendig file {:?} transcoded - remaining slots {}/{}",
            &full_path,
            transcoding.max_transcodings - running_transcodings - 1,
            transcoding.max_transcodings
        );
        serve_file_transcoded(full_path, seek, span, transcoding_quality, &counter)
    }
}

fn serve_file_transcoded(
    full_path: AudioFilePath<PathBuf>,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    transcoding_quality: QualityLevel,
    counter: &Counter,
) -> ResponseFuture {
    let transcoder = get_config().transcoder(transcoding_quality);
    let params = transcoder.transcoding_params();
    let mime = if let QualityLevel::Passthrough = transcoding_quality {
        guess_format(full_path.as_ref()).mime
    } else {
        transcoder.transcoded_mime()
    };

    let fut = transcoder
        .transcode(full_path, seek, span, counter.clone(), transcoding_quality)
        .then(move |res| match res {
            Ok(stream) => future::ok(
                HyperResponse::builder()
                    .typed_header(ContentType::from(mime))
                    .header("X-Transcode", params.as_bytes())
                    .body(Body::wrap_stream(stream.map_err(Error::new)))
                    .unwrap(),
            ),
            Err(e) => {
                error!("Cannot create transcoded stream, error: {}", e);
                future::ok(short_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    SEVER_ERROR_TRANSCODING,
                ))
            }
        });
    Box::pin(fut)
}

pub struct ChunkStream<T> {
    src: Option<T>,
    remains: u64,
    buf: [u8; 8 * 1024],
}

impl<T: AsyncRead + Unpin> Stream for ChunkStream<T> {
    type Item = Result<Vec<u8>, io::Error>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let pin = self.get_mut();
        if let Some(ref mut src) = pin.src {
            if pin.remains == 0 {
                pin.src.take();
                return Poll::Ready(None);
            }
            let mut buf = ReadBuf::new(&mut pin.buf[..]);
            match ready! {
                {
                let pinned_stream = Pin::new(src);
                pinned_stream.poll_read(ctx, &mut buf)
                }
            } {
                Ok(_) => {
                    let read = buf.filled().len();
                    if read == 0 {
                        pin.src.take();
                        Poll::Ready(None)
                    } else {
                        let to_send = pin.remains.min(read as u64);
                        pin.remains -= to_send;
                        let chunk = pin.buf[..to_send as usize].to_vec();
                        Poll::Ready(Some(Ok(chunk)))
                    }
                }
                Err(e) => Poll::Ready(Some(Err(e))),
            }
        } else {
            error!("Polling after stream is done");
            Poll::Ready(None)
        }
    }
}

impl<T: AsyncRead> ChunkStream<T> {
    pub fn new(src: T) -> Self {
        ChunkStream::new_with_limit(src, std::u64::MAX)
    }
    pub fn new_with_limit(src: T, remains: u64) -> Self {
        ChunkStream {
            src: Some(src),
            remains,
            buf: [0u8; 8 * 1024],
        }
    }
}

async fn serve_opened_file(
    mut file: tokio::fs::File,
    range: Option<ByteRange>,
    caching: Option<u32>,
    mime: mime::Mime,
) -> Result<Response, io::Error> {
    let meta = file.metadata().await?;
    let file_len = meta.len();
    if file_len == 0 {
        warn!("File has zero size ")
    }
    let last_modified = meta.modified().ok();
    let mut resp = HyperResponse::builder().typed_header(ContentType::from(mime));
    if let Some(age) = caching {
        let cache = CacheControl::new()
            .with_public()
            .with_max_age(std::time::Duration::from_secs(u64::from(age)));
        resp = resp.typed_header(cache);
        if let Some(last_modified) = last_modified {
            resp = resp.typed_header(LastModified::from(last_modified));
        }
    }

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
                (0, checked_dec(file_len))
            }
        },
        None => {
            resp = resp
                .status(StatusCode::OK)
                .typed_header(AcceptRanges::bytes());
            (0, checked_dec(file_len))
        }
    };
    let _pos = file.seek(SeekFrom::Start(start)).await;
    let sz = end - start + 1;
    let stream = ChunkStream::new_with_limit(file, sz);
    let resp = resp
        .typed_header(ContentLength(sz))
        .body(Body::wrap_stream(stream))
        .unwrap();
    Ok(resp)
}

fn serve_file_from_fs(
    full_path: &Path,
    range: Option<ByteRange>,
    caching: Option<u32>,
) -> ResponseFuture {
    let filename: PathBuf = full_path.into();
    let fut = async move {
        match tokio::fs::File::open(&filename).await {
            Ok(file) => {
                let mime = guess_mime_type(&filename);
                serve_opened_file(file, range, caching, mime)
                    .await
                    .map_err(Error::new)
            }
            Err(e) => {
                error!("Error when sending file {:?} : {}", filename, e);
                Ok(short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE))
            }
        }
    };
    Box::pin(fut)
}

pub fn send_file_simple<P: AsRef<Path>>(
    base_path: &'static Path,
    file_path: P,
    cache: Option<u32>,
) -> ResponseFuture {
    let full_path = base_path.join(&file_path);
    serve_file_from_fs(&full_path, None, cache)
}

pub fn send_file<P: AsRef<Path>>(
    base_path: &'static Path,
    file_path: P,
    range: Option<ByteRange>,
    seek: Option<f32>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: Option<QualityLevel>,
) -> ResponseFuture {
    let (real_path, span) = parse_chapter_path(file_path.as_ref());
    let full_path = base_path.join(real_path);
    if let Some(transcoding_quality) = transcoding_quality {
        debug!(
            "Sending file transcoded in quality {:?}",
            transcoding_quality
        );
        serve_file_cached_or_transcoded(
            full_path,
            seek,
            span,
            range,
            transcoding,
            transcoding_quality,
        )
    } else if span.is_some() {
        debug!("Sending part of file remuxed");
        serve_file_transcoded_checked(
            AudioFilePath::Original(full_path),
            seek,
            span,
            transcoding,
            QualityLevel::Passthrough,
        )
    } else {
        debug!("Sending file directly from fs");
        serve_file_from_fs(&full_path, range, None)
    }
}

pub fn get_folder(
    base_path: &'static Path,
    folder_path: PathBuf,
    ordering: FoldersOrdering,
) -> ResponseFuture {
    Box::pin(
        blocking(move || list_dir(&base_path, &folder_path, ordering))
            .map_ok(|res| match res {
                Ok(folder) => json_response(&folder),
                Err(_) => short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE),
            })
            .map_err(Error::new),
    )
}

#[cfg(not(feature = "folder-download"))]
pub fn download_folder(_base_path: &'static Path, _folder_path: PathBuf) -> ResponseFuture {
    unimplemented!();
}

#[cfg(feature = "folder-download")]
pub fn download_folder(base_path: &'static Path, folder_path: PathBuf) -> ResponseFuture {
    let full_path = base_path.join(&folder_path);
    let f = tokio::fs::metadata(full_path.clone())
        .map_err(|e| {
            error!("Cannot get meta for download path");
            Error::new(e).context("metadata for folder download")
        })
        .and_then(move |meta| {
            if meta.is_file() {
                serve_file_from_fs(&full_path, None, None)
            } else {
                let mut download_name = folder_path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .map(std::borrow::ToOwned::to_owned)
                    .unwrap_or_else(|| "audio".into());
                download_name.push_str(".tar");
                let fut = blocking(move || list_dir_files_only(&base_path, &folder_path))
                    .map_ok(move |res| match res {
                        Ok(folder) => {
                            let total_len: u64;
                            {
                                let lens_iter = (&folder).iter().map(|i| i.1);
                                total_len = async_tar::calc_size(lens_iter);
                            }
                            debug!("Total len of folder is {}", total_len);
                            let files = folder.into_iter().map(|i| i.0);
                            let tar_stream = async_tar::TarStream::tar_iter(files);
                            let disposition = format!("attachment; filename=\"{}\"", download_name);
                            HyperResponse::builder()
                                .typed_header(ContentType::from(
                                    "application/x-tar".parse::<mime::Mime>().unwrap(),
                                ))
                                .typed_header(ContentLength(total_len))
                                .header(CONTENT_DISPOSITION, disposition.as_bytes())
                                .body(Body::wrap_stream(tar_stream))
                                .unwrap()
                        }
                        Err(e) => {
                            error!("Cannot list download dir: {}", e);
                            short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE)
                        }
                    })
                    .map_err(|e| {
                        error!("Error listing files for tar: {}", e);
                        Error::new(e).context("listing files for tar")
                    });

                Box::pin(fut)
            }
        });
    Box::pin(f)
}

fn json_response<T: serde::Serialize>(data: &T) -> Response {
    let json = serde_json::to_string(data).expect("Serialization error");

    HyperResponse::builder()
        .typed_header(ContentType::json())
        .typed_header(ContentLength(json.len() as u64))
        .body(json.into())
        .unwrap()
}

const UKNOWN_NAME: &str = "unknown";

pub fn collections_list() -> ResponseFuture {
    let collections = Collections {
        folder_download: !get_config().disable_folder_download,
        count: get_config().base_dirs.len() as u32,
        names: get_config()
            .base_dirs
            .iter()
            .map(|p| p.file_name().and_then(OsStr::to_str).unwrap_or(UKNOWN_NAME))
            .collect(),
    };
    Box::pin(future::ok(json_response(&collections)))
}

pub fn transcodings_list() -> ResponseFuture {
    let transcodings = Transcodings::new();
    Box::pin(future::ok(json_response(&transcodings)))
}

pub fn search(
    collection: usize,
    searcher: Search<String>,
    query: String,
    ordering: FoldersOrdering,
) -> ResponseFuture {
    Box::pin(
        blocking(move || {
            let res = searcher.search(collection, query, ordering);
            json_response(&res)
        })
        .map_err(Error::new),
    )
}

pub fn recent(collection: usize, searcher: Search<String>) -> ResponseFuture {
    Box::pin(
        blocking(move || {
            let res = searcher.recent(collection);
            json_response(&res)
        })
        .map_err(Error::new),
    )
}
