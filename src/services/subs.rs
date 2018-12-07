use super::get_real_file_type;
use super::search::{Search, SearchTrait};
use super::transcode::{QualityLevel, Transcoder};
use super::types::*;
use super::Counter;
use config::get_config;
use error::Error;
use futures::future::{self, poll_fn, Future};
use futures::{Async, Stream};
use hyper::header::{
    HeaderValue, ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    LAST_MODIFIED,
};
use hyper::{Body, Response as HyperResponse, StatusCode};
use hyperx::header::{CacheControl, CacheDirective, ContentRange, ContentRangeSpec, LastModified};
use mime;
use mime_guess::guess_mime_type;
use serde_json;
use std::fs;
use std::io::{self, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use taglib;
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
    serve_file_transcoded_checked(full_path, seek, transcoding, transcoding_quality)
}

#[cfg(feature = "transcoding-cache")]
fn serve_file_cached_or_transcoded(
    full_path: PathBuf,
    seek: Option<f32>,
    range: Option<::hyperx::header::ByteRangeSpec>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: QualityLevel,
) -> ResponseFuture {
    use crate::cache::{cache_key, get_cache};
    let cache = get_cache();
    let cache_key = cache_key(&full_path, &transcoding_quality);
    let fut = cache
        .get_async(cache_key)
        .then(|res| match res {
            Err(e) => {
                error!("Cache lookup error: {}", e);
                Ok(None)
            }
            Ok(f) => Ok(f),
        })
        .and_then(move |maybe_file| match maybe_file {
            None => {
                serve_file_transcoded_checked(full_path, seek, transcoding, transcoding_quality)
            }
            Some(f) => {
                debug!("Sending file {:?} from transcoded cache", &full_path);
                Box::new(
                    serve_opened_file(f, range, None, Transcoder::transcoded_mime())
                    .map_err(|e| {
                        error!("Error sending cached file: {}", e);
                        Error::new_with_cause(e)
                        })
                    )
            }
        });

    Box::new(fut)
}

fn serve_file_transcoded_checked(
    full_path: PathBuf,
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
    full_path: PathBuf,
    seek: Option<f32>,
    transcoding_quality: QualityLevel,
    counter: &Counter,
) -> ResponseFuture {
    let transcoder = Transcoder::new(get_config().transcoding.get(transcoding_quality));
    match transcoder.transcode(full_path, seek, counter) {
        Ok(stream) => {
            let resp = HyperResponse::builder()
                .header(CONTENT_TYPE, Transcoder::transcoded_mime().as_ref())
                .header("X-Transcode", transcoder.transcoding_params().as_bytes())
                .body(Body::wrap_stream(stream.map_err(Error::new_with_cause)))
                .unwrap();

            Box::new(future::ok(resp))
        }
        Err(e) => {
            error!("Cannot create transcoded stream, error: {}", e);
            Box::new(future::ok(short_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                SEVER_ERROR_TRANSCODING,
            )))
        }
    }
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

        let read = try_ready!{self.src.as_mut().unwrap().poll_read(&mut self.buf)};
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

fn list_dir<P: AsRef<Path>, P2: AsRef<Path>>(
    base_dir: P,
    dir_path: P2,
) -> Result<AudioFolder, io::Error> {
    fn os_to_string(s: ::std::ffi::OsString) -> String {
        match s.into_string() {
            Ok(s) => s,
            Err(s) => {
                warn!("Invalid file name - cannot covert to UTF8 : {:?}", s);
                "INVALID_NAME".into()
            }
        }
    }

    let full_path = base_dir.as_ref().join(&dir_path);
    match fs::read_dir(&full_path) {
        Ok(dir_iter) => {
            let mut files = vec![];
            let mut subfolders = vec![];
            let mut cover = None;
            let mut description = None;
            let allow_symlinks = get_config().allow_symlinks;

            for item in dir_iter {
                match item {
                    Ok(f) => match get_real_file_type(&f, &full_path, allow_symlinks) {
                        Ok(ft) => {
                            let path = f.path().strip_prefix(&base_dir).unwrap().into();
                            if ft.is_dir() {
                                subfolders.push(AudioFolderShort {
                                    path,
                                    name: os_to_string(f.file_name()),
                                })
                            } else if ft.is_file() {
                                if is_audio(&path) {
                                    let mime = ::mime_guess::guess_mime_type(&path);
                                    let meta = get_audio_properties(&base_dir.as_ref().join(&path));
                                    files.push(AudioFile {
                                        meta,
                                        path,
                                        name: os_to_string(f.file_name()),

                                        mime: mime.to_string(),
                                    })
                                } else if cover.is_none() && is_cover(&path) {
                                    cover = Some(TypedFile::new(path))
                                } else if description.is_none() && is_description(&path) {
                                    description = Some(TypedFile::new(path))
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Cannot get dir entry type for {:?}, error: {}", f.path(), e)
                        }
                    },
                    Err(e) => warn!(
                        "Cannot list items in directory {:?}, error {}",
                        dir_path.as_ref().as_os_str(),
                        e
                    ),
                }
            }
            files.sort_unstable_by_key(|e| e.name.to_uppercase());
            subfolders.sort_unstable_by_key(|e| e.name.to_uppercase());;
            Ok(AudioFolder {
                files,
                subfolders,
                cover,
                description,
            })
        }
        Err(e) => {
            error!(
                "Requesting wrong directory {:?} : {}",
                (&full_path).as_os_str(),
                e
            );
            Err(e)
        }
    }
}

pub fn get_audio_properties(audio_file_name: &Path) -> Option<AudioMeta> {
    let filename = audio_file_name.as_os_str().to_str();
    match filename {
        Some(fname) => {
            let audio_file = taglib::File::new(fname);
            match audio_file {
                Ok(f) => match f.audioproperties() {
                    Ok(ap) => {
                        return Some(AudioMeta {
                            duration: ap.length(),
                            bitrate: {
                                let mut bitrate = ap.bitrate();
                                let duration = ap.length();
                                if bitrate == 0 && duration != 0 {
                                    // estimate from duration and file size
                                    // Will not work well for small files
                                    if let Ok(size) = audio_file_name.metadata().map(|m| m.len()) {
                                        bitrate = (size * 8 / u64::from(duration) / 1024) as u32;
                                        debug!("Estimating bitrate to {}", bitrate);
                                    };
                                }
                                bitrate
                            },
                        });
                    }
                    Err(e) => warn!("File {} does not have audioproperties {:?}", fname, e),
                },
                Err(e) => warn!("Cannot get audiofile {} error {:?}", fname, e),
            }
        }
        None => warn!("File name {:?} is not utf8", filename),
    };

    None
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use config::init_default_config;
    use serde_json;

    #[test]
    fn test_list_dir() {
        init_default_config();
        let res = list_dir("/non-existent", "folder");
        assert!(res.is_err());
        let res = list_dir("./", "test_data/");
        assert!(res.is_ok());
        let folder = res.unwrap();
        assert_eq!(folder.files.len(), 3);
        assert!(folder.cover.is_some());
        assert!(folder.description.is_some());
    }

    #[test]
    fn test_json() {
        init_default_config();
        let folder = list_dir("./", "test_data/").unwrap();
        let json = serde_json::to_string(&folder).unwrap();
        println!("JSON: {}", &json);
    }

    #[test]
    fn test_meta() {
        let res = get_audio_properties(Path::new("./test_data/01-file.mp3"));
        assert!(res.is_some());
        let meta = res.unwrap();
        assert_eq!(meta.bitrate, 220);
        assert_eq!(meta.duration, 2);
    }

}
