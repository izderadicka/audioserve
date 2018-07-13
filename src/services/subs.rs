use super::get_real_file_type;
use super::search::{Search, SearchTrait};
use super::transcode::{QualityLevel, Transcoder};
use super::types::*;
use super::Counter;
use config::get_config;
use error::Error;
use futures::future::{self, Future};
use futures::sync::{mpsc, oneshot};
use futures::{Sink, Stream};
use hyper::header::{
    HeaderValue, ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    LAST_MODIFIED,
};
use hyper::{Body, Chunk, Response as HyperResponse, StatusCode};
use hyperx::header::{CacheControl, CacheDirective, ContentRange, ContentRangeSpec, LastModified};
use mime;
use mime_guess::guess_mime_type;
use serde_json;
use simple_thread_pool::Pool;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::thread;
use taglib;

const BUF_SIZE: usize = 8 * 1024;
pub const NOT_FOUND_MESSAGE: &str = "Not Found";
const THREAD_SEND_ERROR: &str = "Cannot communicate with other thread";

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

struct GuardedCounter(Counter);
impl Drop for GuardedCounter {
    fn drop(&mut self) {
        if thread::panicking() {
            error!("Worker thread panicked")
        }
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn guarded_spawn<F>(counter: Counter, f: F) -> thread::JoinHandle<()>
where
    F: FnOnce() -> () + Send + 'static,
{
    counter.fetch_add(1, Ordering::SeqCst);
    let gc = GuardedCounter(counter);
    thread::spawn(move || {
        f();
        drop(gc);
        //counter.fetch_sub(1, Ordering::SeqCst);
    })
}

fn serve_file_transcoded(
    full_path: &Path,
    seek: Option<f32>,
    transcoder: &Transcoder,
    tx: ::futures::sync::oneshot::Sender<Response>,
) {
    let (body_tx, body_rx) = mpsc::channel(1);
    let resp = HyperResponse::builder()
        .header(CONTENT_TYPE, transcoder.transcoded_mime().as_ref())
        .header("X-Transcode", transcoder.transcoding_params().as_bytes())
        .body(Body::wrap_stream(body_rx.map_err(|_e| Error::new())))
        .unwrap();
    tx.send(resp).expect(THREAD_SEND_ERROR);

    transcoder.transcode(full_path, seek, body_tx);
}

fn serve_file_from_fs(
    full_path: &Path,
    range: Option<::hyperx::header::ByteRangeSpec>,
    caching: Option<u32>,
    tx: ::futures::sync::oneshot::Sender<Response>,
) {
    match File::open(full_path) {
        Ok(mut file) => {
            let (mut body_tx, body_rx) = mpsc::channel(1);
            let meta = file.metadata().expect("File stat error");
            let file_sz = meta.len();
            let last_modified = meta.modified().ok();
            let mime = guess_mime_type(&full_path);
            let mut res = HyperResponse::builder()
                .header(CONTENT_TYPE, mime.as_ref())
                .body(Body::wrap_stream(body_rx.map_err(|_e| Error::new())))
                .unwrap();

            let range = range
                .and_then(|r| r.to_satisfiable_range(file_sz))
                .map(|(s, e)| {
                    assert!(e >= s);
                    (s, e, e - s + 1)
                });

            let (start, content_len) = match range {
                Some((s, e, l)) => {
                    let range = ContentRange(ContentRangeSpec::Bytes {
                        range: Some((s, e)),
                        instance_length: Some(file_sz),
                    });
                    res.headers_mut().insert(
                        CONTENT_RANGE,
                        HeaderValue::from_str(&format!("{}", range)).unwrap(),
                    );
                    *res.status_mut() = StatusCode::PARTIAL_CONTENT;
                    (s, l)
                }
                None => {
                    res.headers_mut()
                        .insert(ACCEPT_RANGES, HeaderValue::from_str("bytes").unwrap());
                    (0, file_sz)
                }
            };

            res.headers_mut().insert(
                CONTENT_LENGTH,
                HeaderValue::from_str(&format!("{}", content_len)).unwrap(),
            );
            if let Some(age) = caching {
                let cache = CacheControl(vec![CacheDirective::Public, CacheDirective::MaxAge(age)]);
                res.headers_mut().insert(
                    CACHE_CONTROL,
                    HeaderValue::from_str(&format!("{}", cache)).unwrap(),
                );
                if let Some(last_modified) = last_modified {
                    let lm = LastModified(last_modified.into());
                    res.headers_mut().insert(
                        LAST_MODIFIED,
                        HeaderValue::from_str(&format!("{}", lm)).unwrap(),
                    );
                }
            }

            tx.send(res).expect(THREAD_SEND_ERROR);
            let mut buf = [0u8; BUF_SIZE];
            if start > 0 {
                file.seek(SeekFrom::Start(start)).expect("Seek error");
            }
            let mut remains = content_len as usize;
            loop {
                match file.read(&mut buf) {
                    Ok(n) => if n == 0 {
                        trace!("Received 0");
                        body_tx.close().expect(THREAD_SEND_ERROR);
                        break;
                    } else {
                        let to_send = n.min(remains);
                        trace!("Received {}, remains {}, sending {}", n, remains, to_send);
                        let slice = buf[..to_send].to_vec();
                        let c: Chunk = slice.into();
                        match body_tx.send(c).wait() {
                            Ok(t) => body_tx = t,
                            Err(_) => break,
                        };

                        if remains <= n {
                            trace!("All send");
                            body_tx.close().expect(THREAD_SEND_ERROR);
                            break;
                        } else {
                            remains -= n
                        }
                    },

                    Err(e) => {
                        error!("Sending file error {}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => {
            error!("File opening error {}", e);
            tx.send(short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE))
                .expect(THREAD_SEND_ERROR);
        }
    }
}

macro_rules! spawn_in_pool {
    ($pool:ident, $tx:ident, $rx:ident, $f:expr) => {
        let ($tx, $rx) = oneshot::channel();
        if $pool.spawn($f).is_err() {
            return short_response_boxed(
                StatusCode::SERVICE_UNAVAILABLE,
                super::OVERLOADED_MESSAGE,
            );
        }
        return box_rx($rx);
    };
}

pub fn send_file_simple(
    base_path: &'static Path,
    file_path: PathBuf,
    cache: Option<u32>,
    mut pool: Pool,
) -> ResponseFuture {
    spawn_in_pool!(pool, tx, rx, move || {
        let full_path = base_path.join(&file_path);
        if full_path.exists() {
            serve_file_from_fs(&full_path, None, cache, tx);
        } else {
            error!("File {:?} does not exists", full_path);
            tx.send(short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE))
                .expect(THREAD_SEND_ERROR);
        }
    });
}

pub fn send_file(
    base_path: &'static Path,
    file_path: PathBuf,
    range: Option<::hyperx::header::ByteRangeSpec>,
    seek: Option<f32>,
    mut pool: Pool,
    transcoding: super::TranscodingDetails,
    transcoding_quality: Option<QualityLevel>,
) -> ResponseFuture {
    spawn_in_pool!(pool, tx, rx, move || {
        let full_path = base_path.join(&file_path);
        if full_path.exists() {
            if transcoding_quality.is_some() {
                debug!(
                    "Sending file transcoded in quality {:?}",
                    transcoding_quality
                );
                let counter = transcoding.transcodings;
                let transcoder =
                    Transcoder::new(get_config().transcoding.get(transcoding_quality.unwrap()));
                let running_transcodings = counter.load(Ordering::SeqCst);
                if running_transcodings >= transcoding.max_transcodings {
                    warn!("Max transcodings reached {}", transcoding.max_transcodings);
                    tx.send(short_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Max transcodings reached",
                    )).expect(THREAD_SEND_ERROR)
                } else {
                    debug!(
                        "Sendig file {:?} transcoded - remaining slots {}/{}",
                        &full_path,
                        transcoding.max_transcodings - running_transcodings - 1,
                        transcoding.max_transcodings
                    );
                    guarded_spawn(counter, move || {
                        serve_file_transcoded(&full_path, seek, &transcoder, tx)
                    });
                }
            } else {
                debug!("Sending file directly from fs");
                serve_file_from_fs(&full_path, range, None, tx);
            }
        } else {
            error!("File {:?} does not exists", full_path);
            tx.send(short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE))
                .expect(THREAD_SEND_ERROR);
        }
    });
}

fn box_rx(rx: ::futures::sync::oneshot::Receiver<Response>) -> ResponseFuture {
    Box::new(rx.map_err(Error::new_with_cause))
}

pub fn get_folder(
    base_path: &'static Path,
    folder_path: PathBuf,
    mut pool: Pool,
) -> ResponseFuture {
    spawn_in_pool!(pool, tx, rx, move || {
        match list_dir(&base_path, &folder_path) {
            Ok(folder) => {
                tx.send(json_response(&folder)).expect(THREAD_SEND_ERROR);
            }
            Err(_) => {
                tx.send(short_response(StatusCode::NOT_FOUND, NOT_FOUND_MESSAGE))
                    .expect(THREAD_SEND_ERROR);
            }
        }
    });
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

                                        mime: format!("{}", mime),
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

fn json_response<T: ::serde::Serialize>(data: &T) -> Response {
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

pub fn search(
    base_dir: &'static Path,
    searcher: Search,
    query: String,
    mut pool: Pool,
) -> ResponseFuture {
    spawn_in_pool!(pool, tx, rx, move || {
        let res = searcher.search(base_dir, query);
        tx.send(json_response(&res)).expect(THREAD_SEND_ERROR);
    });
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_guarded_spawn() {
        let c = Arc::new(AtomicUsize::new(0));
        let c2 = c.clone();
        guarded_spawn(c.clone(), move || {
            println!("hey");
            assert_eq!(c2.load(Ordering::SeqCst), 1)
        }).join()
            .unwrap();

        assert_eq!(c.load(Ordering::SeqCst), 0);

        let res = guarded_spawn(c.clone(), || {
            println!("Will panic");
            panic!("panic");
        }).join();
        assert!(res.is_err());

        assert_eq!(c.load(Ordering::SeqCst), 0)
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
