use super::search::{Search, SearchTrait};
use super::transcode::{AudioFilePath, QualityLevel};
use super::audio_folder::list_dir;
#[cfg(feature="folder-download")]
use super::audio_folder::list_dir_files_only;
use super::types::*;
use super::Counter;
use config::get_config;
use error::Error;
use futures::future::{self, poll_fn, Future};
use futures::{Async, Stream};
#[cfg(feature = "folder-download")]
use hyper::header::CONTENT_DISPOSITION;
use hyper::header::{
    HeaderValue, ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    LAST_MODIFIED,
};
use hyper::{Body, Response as HyperResponse, StatusCode};
use hyperx::header::{CacheControl, CacheDirective, ContentRange, ContentRangeSpec, LastModified};
#[cfg(feature = "folder-download")]
use hyperx::header::{Charset, ContentDisposition, DispositionParam, DispositionType};
use mime;
use mime_guess::guess_mime_type;
use serde_json;
use std::io::{self, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tokio::io::AsyncRead;
use tokio_threadpool::blocking;

pub const NOT_FOUND_MESSAGE: &str = "Not Found";
const SEVER_ERROR_TRANSCODING: &str = "Server error during transcoding process";

type Response = HyperResponse<Body>;

pub type ResponseFuture = Box<Future<Item = Response, Error = Error> + Send>;

pub fn short_response(status: StatusCode, msg: &'static str) -> Response {
    HyperResponse::builder()
        .status(status)
        .header(CONTENT_LENGTH, msg.len())
        .body(msg.into())
        .unwrap()
}

pub fn short_response_boxed(status: StatusCode, msg: &'static str) -> ResponseFuture {
    Box::new(future::ok(short_response(status, msg)))
}

#[cfg(not(feature = "transcoding-cache"))]
fn serve_file_cached_or_transcoded(
    full_path: PathBuf,
    seek: Option<f32>,
    _range: Option<::hyperx::header::ByteRangeSpec>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: QualityLevel,
) -> ResponseFuture {
    serve_file_transcoded_checked(
        AudioFilePath::Original(full_path),
        seek,
        transcoding,
        transcoding_quality,
    )
}

#[cfg(feature = "transcoding-cache")]
fn serve_file_cached_or_transcoded(
    full_path: PathBuf,
    seek: Option<f32>,
    range: Option<::hyperx::header::ByteRangeSpec>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: QualityLevel,
) -> ResponseFuture {
    if get_config().transcoding_cache.disabled {
        return Box::new(serve_file_transcoded_checked(
            AudioFilePath::Original(full_path),
            seek,
            transcoding,
            transcoding_quality,
        ));
    }

    use crate::cache::{cache_key, get_cache};
    let cache = get_cache();
    let cache_key = cache_key(&full_path, &transcoding_quality);
    let fut = cache
        .get_async2(cache_key)
        .then(|res| match res {
            Err(e) => {
                error!("Cache lookup error: {}", e);
                Ok(None)
            }
            Ok(f) => Ok(f),
        })
        .and_then(move |maybe_file| match maybe_file {
            None => serve_file_transcoded_checked(
                AudioFilePath::Original(full_path),
                seek,
                transcoding,
                transcoding_quality,
            ),
            Some((f, path)) => {
                if seek.is_some() {
                    debug!("File is in cache, but time seek is required");
                    serve_file_transcoded_checked(
                        AudioFilePath::Transcoded(path),
                        seek,
                        transcoding,
                        transcoding_quality,
                    )
                } else {
                    debug!("Sending file {:?} from transcoded cache", &full_path);
                    let mime = get_config()
                        .transcoder(transcoding_quality)
                        .transcoded_mime();
                    Box::new(serve_opened_file(f, range, None, mime).map_err(|e| {
                        error!("Error sending cached file: {}", e);
                        Error::new_with_cause(e)
                    }))
                }
            }
        });

    Box::new(fut)
}

fn serve_file_transcoded_checked(
    full_path: AudioFilePath<PathBuf>,
    seek: Option<f32>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: QualityLevel,
) -> ResponseFuture {
    let counter = transcoding.transcodings;

    let running_transcodings = counter.load(Ordering::SeqCst);
    if running_transcodings >= transcoding.max_transcodings {
        warn!("Max transcodings reached {}", transcoding.max_transcodings);
        Box::new(future::ok(short_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "Max transcodings reached",
        )))
    } else {
        debug!(
            "Sendig file {:?} transcoded - remaining slots {}/{}",
            &full_path,
            transcoding.max_transcodings - running_transcodings - 1,
            transcoding.max_transcodings
        );
        serve_file_transcoded(full_path, seek, transcoding_quality, &counter)
    }
}

fn serve_file_transcoded(
    full_path: AudioFilePath<PathBuf>,
    seek: Option<f32>,
    transcoding_quality: QualityLevel,
    counter: &Counter,
) -> ResponseFuture {
    let transcoder = get_config().transcoder(transcoding_quality);
    let params = transcoder.transcoding_params();
    let mime = transcoder.transcoded_mime();
    let fut = transcoder
        .transcode(full_path, seek, counter.clone(), transcoding_quality)
        .then(move |res| match res {
            Ok(stream) => {
                let resp = HyperResponse::builder()
                    .header(CONTENT_TYPE, mime.as_ref())
                    .header("X-Transcode", params.as_bytes())
                    .body(Body::wrap_stream(stream.map_err(Error::new_with_cause)))
                    .unwrap();

                Ok(resp)
            }
            Err(e) => {
                error!("Cannot create transcoded stream, error: {}", e);
                Ok(short_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    SEVER_ERROR_TRANSCODING,
                ))
            }
        });
    Box::new(fut)
}

pub struct ChunkStream<T> {
    src: Option<T>,
    remains: u64,
    buf: [u8; 8 * 1024],
}

impl<T: AsyncRead> Stream for ChunkStream<T> {
    type Item = Vec<u8>;
    type Error = io::Error;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        if self.src.is_none() {
            error!("Polling after stream is done");
            return Ok(Async::Ready(None));
        }
        if self.remains == 0 {
            self.src.take();
            return Ok(Async::Ready(None));
        }

        let read = try_ready! {self.src.as_mut().unwrap().poll_read(&mut self.buf)};
        if read == 0 {
            self.src.take();
            Ok(Async::Ready(None))
        } else {
            let to_send = self.remains.min(read as u64);
            self.remains -= to_send;
            let chunk = self.buf[..to_send as usize].to_vec();
            Ok(Async::Ready(Some(chunk)))
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

fn serve_opened_file(
    file: tokio::fs::File,
    range: Option<::hyperx::header::ByteRangeSpec>,
    caching: Option<u32>,
    mime: mime::Mime,
) -> impl Future<Item = Response, Error = io::Error> {
    file.metadata().and_then(move |(file, meta)| {
        let file_len = meta.len();
        if file_len == 0 {
            warn!("File has zero size ")
        }
        let last_modified = meta.modified().ok();
        let mut resp = HyperResponse::builder();
        resp.header(CONTENT_TYPE, mime.as_ref());
        if let Some(age) = caching {
            let cache = CacheControl(vec![CacheDirective::Public, CacheDirective::MaxAge(age)]);
            resp.header(CACHE_CONTROL, cache.to_string().as_bytes());
            if let Some(last_modified) = last_modified {
                let lm = LastModified(last_modified.into());
                resp.header(LAST_MODIFIED, lm.to_string().as_bytes());
            }
        }

        fn checked_dec(x: u64) -> u64 {
            if x > 0 {
                x - 1
            } else {
                x
            }
        }

        let (start, end) = match range {
            Some(range) => match range.to_satisfiable_range(file_len) {
                Some(l) => {
                    resp.status(StatusCode::PARTIAL_CONTENT);
                    let h = ContentRange(ContentRangeSpec::Bytes {
                        range: Some((l.0, l.1)),
                        instance_length: Some(file_len),
                    });
                    resp.header(
                        CONTENT_RANGE,
                        HeaderValue::from_str(&h.to_string()).unwrap(),
                    );
                    l
                }
                None => {
                    error!("Wrong range {}", range);
                    (0, checked_dec(file_len))
                }
            },
            None => {
                resp.status(StatusCode::OK);
                resp.header(ACCEPT_RANGES, "bytes");
                (0, checked_dec(file_len))
            }
        };
        file.seek(SeekFrom::Start(start)).map(move |(file, _pos)| {
            let stream = ChunkStream::new_with_limit(file, end - start + 1);
            resp.header(CONTENT_LENGTH, end - start + 1)
                .body(Body::wrap_stream(stream))
                .unwrap()
        })
    })
}

fn serve_file_from_fs(
    full_path: &Path,
    range: Option<::hyperx::header::ByteRangeSpec>,
    caching: Option<u32>,
) -> ResponseFuture {
    let filename: PathBuf = full_path.into(); // we need to copy for lifetime issues as File::open and closures require 'static lifetime
    let filename2: PathBuf = full_path.into();
    let filename3: PathBuf = full_path.into();
    Box::new(
        tokio::fs::File::open(filename)
            .and_then(move |file| {
                let mime = guess_mime_type(filename2);
                serve_opened_file(file, range, caching, mime)
            })
            .or_else(move |_| {
                error!("Error when sending file {:?}", filename3);
                Ok(short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE))
            }),
    )
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
    range: Option<::hyperx::header::ByteRangeSpec>,
    seek: Option<f32>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: Option<QualityLevel>,
) -> ResponseFuture {
    let full_path = base_path.join(&file_path);
    if let Some(transcoding_quality) = transcoding_quality {
        debug!(
            "Sending file transcoded in quality {:?}",
            transcoding_quality
        );
        serve_file_cached_or_transcoded(full_path, seek, range, transcoding, transcoding_quality)
    } else {
        debug!("Sending file directly from fs");
        serve_file_from_fs(&full_path, range, None)
    }
}

pub fn get_folder(base_path: &'static Path, folder_path: PathBuf) -> ResponseFuture {
    Box::new(
        poll_fn(move || blocking(|| list_dir(&base_path, &folder_path)))
            .map(|res| match res {
                Ok(folder) => json_response(&folder),
                Err(_) => short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE),
            })
            .map_err(Error::new_with_cause),
    )
}

#[cfg(not(feature = "folder-download"))]
pub fn download_folder(_base_path: &'static Path, _folder_path: PathBuf) -> ResponseFuture {
    unimplemented!();
}

#[cfg(feature = "folder-download")]
pub fn download_folder(base_path: &'static Path, folder_path: PathBuf) -> ResponseFuture {
    let mut download_name = folder_path
        .file_name()
        .and_then(|fname| fname.to_str())
        .map(|fname| fname.to_owned())
        .unwrap_or_else(|| "audio".into());
    download_name.push_str(".tar");
    let f = poll_fn(move || blocking(|| list_dir_files_only(&base_path, &folder_path)))
        .map(move |res| match res {
            Ok(folder) => {
                let total_len: u64;
                {
                    let lens_iter = (&folder).iter().map(|i| i.1);
                    total_len = async_tar::calc_size(lens_iter);
                }
                debug!("Total len of folder is {}", total_len);
                let files = folder.into_iter().map(|i| i.0);
                let tar_stream = async_tar::TarStream::tar_iter_rel(files, base_path);
                let mut resp = HyperResponse::builder();
                resp.header(CONTENT_TYPE, "application/x-tar");
                resp.header(CONTENT_LENGTH, total_len);
                let disposition = ContentDisposition {
                    disposition: DispositionType::Attachment,
                    parameters: vec![DispositionParam::Filename(
                        Charset::Ext("UTF-8".into()),
                        None,
                        download_name.into(),
                    )],
                };
                resp.header(CONTENT_DISPOSITION, disposition.to_string().as_bytes());
                resp.body(Body::wrap_stream(tar_stream)).unwrap()
            }
            Err(e) => {
                error!("Cannot list download dir: {}", e);
                short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE)
            }
        })
        .map_err(|e| {
            error!("Error listing files for tar: {}", e);
            Error::new_with_cause(e)
        });

    Box::new(f)
}

// pub fn download_folder_old(base_path: &'static Path, folder_path: PathBuf) -> ResponseFuture {
//     let mut download_name = folder_path.file_name()
//             .and_then(|fname| fname.to_str())
//             .map(|fname| fname.to_owned())
//             .unwrap_or_else(|| "audio".into());
//     download_name.push_str(".tar");
//     let full_path = base_path.join(&folder_path);
//     let tar = async_tar::TarStream::tar_dir(full_path);
//     let f = tar.map(move |tar_stream| {
//         let mut resp = HyperResponse::builder();
//         resp.header(CONTENT_TYPE, "application/x-tar");
//         let disposition = ContentDisposition{
//             disposition: DispositionType::Attachment,
//             parameters: vec![DispositionParam::Filename(
//                 Charset::Ext("UTF-8".into()),
//                 None,
//                 download_name.into()
//             )]
//         };
//         resp.header(CONTENT_DISPOSITION, disposition.to_string().as_bytes());
//         resp.body(Body::wrap_stream(tar_stream)).unwrap()
//     });
//     Box::new(f.map_err(|e| {
//         error!("Cannot create tar because of error {}",e);
//         Error::new_with_cause(e)
//         }))
// }



fn json_response<T: serde::Serialize>(data: &T) -> Response {
    let json = serde_json::to_string(data).expect("Serialization error");
    HyperResponse::builder()
        .header(CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
        .header(CONTENT_LENGTH, json.len())
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
            .map(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(UKNOWN_NAME)
            })
            .collect(),
    };
    Box::new(future::ok(json_response(&collections)))
}

pub fn transcodings_list() -> ResponseFuture {
    let transcodings = Transcodings::new();
    Box::new(future::ok(json_response(&transcodings)))
}

pub fn search(collection: usize, searcher: Search<String>, query: String) -> ResponseFuture {
    Box::new(
        poll_fn(move || {
            let query = query.clone();
            blocking(|| {
                let res = searcher.search(collection, query);
                json_response(&res)
            })
        })
        .map_err(Error::new_with_cause),
    )
}

pub fn recent(collection: usize, searcher: Search<String>) -> ResponseFuture {
    Box::new(
        poll_fn(move || {
            blocking(|| {
                let res = searcher.recent(collection);
                json_response(&res)
            })
        })
        .map_err(Error::new_with_cause),
    )
}


