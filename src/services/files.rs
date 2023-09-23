//#[cfg(feature = "folder-download")]
use super::{
    icon::icon_response,
    response::{
        self, add_cache_headers, fut, not_found, not_found_cached, ChunkStream, ResponseFuture,
    },
    transcode::{guess_format, AudioFilePath, ChosenTranscoding, QualityLevel, Transcoder},
    types::*,
    Counter,
};
use crate::{
    config::get_config,
    error::{Error, Result},
    util::{checked_dec, into_range_bounds, to_satisfiable_range, ResponseBuilderExt},
};
use collection::{
    audio_meta::is_audio, extract_cover, extract_description, guess_mime_type, parse_chapter_path,
    TimeSpan,
};
use futures::prelude::*;
use headers::{AcceptRanges, ContentLength, ContentRange, ContentType};
use hyper::{Body, Response as HyperResponse, StatusCode};
use std::{
    collections::Bound,
    ffi::OsStr,
    io::{self, SeekFrom},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::SystemTime,
};
use tokio::{io::AsyncSeekExt, task::spawn_blocking as blocking};

pub type ByteRange = (Bound<u64>, Bound<u64>);
type Response = HyperResponse<Body>;

#[cfg(not(feature = "transcoding-cache"))]
fn serve_file_cached_or_transcoded(
    full_path: PathBuf,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    _range: Option<ByteRange>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: ChosenTranscoding,
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
async fn serve_file_cached_or_transcoded(
    full_path: PathBuf,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    range: Option<ByteRange>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: ChosenTranscoding,
) -> Result<Response> {
    if get_config().transcoding.cache.disabled {
        return serve_file_transcoded_checked(
            AudioFilePath::Original(full_path),
            seek,
            span,
            transcoding,
            transcoding_quality,
        )
        .await;
    }

    use super::transcode::cache::{cache_key_async, get_cache};
    let cache = get_cache();
    let (cache_key, meta) = cache_key_async(&full_path, &transcoding_quality, span).await?;
    let maybe_file = cache
        .get2(cache_key, meta.into())
        .await
        .unwrap_or_else(|e| {
            error!("Cache lookup error: {}", e);
            None
        });
    match maybe_file {
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
                .await
            } else {
                debug!("Sending file {:?} from transcoded cache", &full_path);
                let mime = transcoding_quality.format.mime();
                serve_opened_file(f, range, None, mime).await.map_err(|e| {
                    error!("Error sending cached file: {}", e);
                    Error::new(e).context("sending cached file")
                })
            }
        }
        None => {
            serve_file_transcoded_checked(
                AudioFilePath::Original(full_path),
                seek,
                span,
                transcoding,
                transcoding_quality,
            )
            .await
        }
    }
}

fn serve_file_transcoded_checked(
    full_path: AudioFilePath<PathBuf>,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: ChosenTranscoding,
) -> ResponseFuture {
    let counter = transcoding.transcodings;
    let mut running_transcodings = counter.load(Ordering::SeqCst);
    loop {
        if running_transcodings >= transcoding.max_transcodings {
            warn!(
                "Max transcodings reached {}/{}",
                running_transcodings, transcoding.max_transcodings
            );
            return response::fut(response::too_many_requests);
        }

        match counter.compare_exchange(
            running_transcodings,
            running_transcodings + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => {
                running_transcodings += 1;
                break;
            }
            Err(curr) => running_transcodings = curr,
        }
    }

    debug!(
        "Sendig file {:?} transcoded - remaining slots {}/{}",
        &full_path,
        transcoding.max_transcodings - running_transcodings,
        transcoding.max_transcodings
    );
    Box::pin(serve_file_transcoded(
        full_path,
        seek,
        span,
        transcoding_quality,
        counter,
    ))
}

async fn serve_file_transcoded(
    full_path: AudioFilePath<PathBuf>,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    transcoding_quality: ChosenTranscoding,
    counter: Counter,
) -> Result<Response> {
    let mime = if let QualityLevel::Passthrough = transcoding_quality.level {
        guess_format(full_path.as_ref()).mime
    } else {
        transcoding_quality.format.mime()
    };

    let transcoder = Transcoder::new(transcoding_quality);
    let params = transcoder.transcoding_params();

    // check if file exists

    if !tokio::fs::metadata(full_path.as_ref())
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
    {
        error!(
            "Requesting non existent file for transcoding {:?}",
            full_path
        );
        return Ok(response::not_found());
    }

    transcoder
        .transcode(full_path, seek, span, counter.clone())
        .await
        .map(move |stream| {
            HyperResponse::builder()
                .typed_header(ContentType::from(mime))
                .header("X-Transcode", params.as_bytes())
                .body(Body::wrap_stream(stream))
                .unwrap()
        })
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
    resp = add_cache_headers(resp, caching, last_modified);

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
                Ok(response::not_found())
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
    transcoding_quality: Option<ChosenTranscoding>,
) -> ResponseFuture {
    let (real_path, span) = parse_chapter_path(file_path.as_ref());
    let full_path = base_path.join(real_path);
    if let Some(transcoding_quality) = transcoding_quality {
        debug!(
            "Sending file transcoded in quality {:?}",
            transcoding_quality.level
        );
        Box::pin(serve_file_cached_or_transcoded(
            full_path,
            seek,
            span,
            range,
            transcoding,
            transcoding_quality,
        ))
    } else if span.is_some() {
        debug!("Sending part of file remuxed");
        serve_file_transcoded_checked(
            AudioFilePath::Original(full_path),
            seek,
            span,
            transcoding,
            ChosenTranscoding::passthough(),
        )
    } else {
        debug!("Sending file directly from fs");
        serve_file_from_fs(&full_path, range, None)
    }
}

async fn send_buffer(
    buf: Vec<u8>,
    mime: mime::Mime,
    cache: Option<u32>,
    last_modified: Option<SystemTime>,
) -> Result<Response, Error> {
    let mut resp = HyperResponse::builder()
        .typed_header(ContentType::from(mime))
        .typed_header(ContentLength(buf.len() as u64))
        .status(StatusCode::OK);
    resp = add_cache_headers(resp, cache, last_modified);

    resp.body(Body::from(buf)).map_err(Error::from)
}

pub fn send_description(
    base_path: &'static Path,
    file_path: impl AsRef<Path>,
    cache: Option<u32>,
) -> ResponseFuture {
    send_data(base_path, file_path, "text/plain", cache, |p| {
        extract_description(p).map(|s| s.into())
    })
}

pub fn send_cover(
    base_path: &'static Path,
    file_path: impl AsRef<Path>,
    cache: Option<u32>,
) -> ResponseFuture {
    send_data(base_path, file_path, "image/jpeg", cache, |p| {
        extract_cover(p)
    })
}

pub fn send_data(
    base_path: &'static Path,
    file_path: impl AsRef<Path>,
    mime: impl AsRef<str> + Send + 'static,
    cache: Option<u32>,
    extractor: impl FnOnce(PathBuf) -> Option<Vec<u8>> + Send + 'static,
) -> ResponseFuture {
    if is_audio(&file_path) {
        // extract description from audio file
        let full_path = base_path.join(file_path);
        let fut = blocking(move || {
            let m = std::fs::metadata(&full_path)
                .and_then(|meta| meta.modified())
                .ok();
            (extractor(full_path), m)
        })
        .map_err(Error::from)
        .and_then(move |(data, last_modified)| match data {
            None => fut(not_found),
            Some(data) => Box::pin(send_buffer(
                data,
                mime.as_ref().parse().unwrap(),
                cache,
                last_modified,
            )),
        });

        Box::pin(fut)
    } else {
        send_file_simple(base_path, file_path, cache)
    }
}

pub fn send_folder_icon(
    collection: usize,
    folder_path: PathBuf,
    collections: Arc<collection::Collections>,
) -> ResponseFuture {
    let r = blocking(
        move || match collections.get_folder_cover_path(collection, folder_path) {
            Ok(Some((p, meta))) => icon_response(p, meta.into()),
            Ok(None) => Ok(not_found_cached(get_config().folder_file_cache_age)),
            Err(e) => {
                error!("error while getting folder icon: {}", e);
                Ok(not_found())
            }
        },
    )
    .map_err(Error::new)
    .then(|f| {
        future::ready(match f {
            Ok(x) => x,
            Err(e) => Err(e),
        })
    });

    Box::pin(r)
}

#[cfg(feature = "folder-download")]
pub fn download_folder(
    base_path: &'static Path,
    folder_path: PathBuf,
    format: DownloadFormat,
    include_subfolders: Option<regex::Regex>,
) -> ResponseFuture {
    use anyhow::Context;
    use hyper::header::CONTENT_DISPOSITION;
    let full_path = base_path.join(&folder_path);
    let f = async move {
        let meta_result = tokio::fs::metadata(&full_path).await;
        let meta = match meta_result {
            Ok(meta) => meta,
            Err(err) => {
                if matches!(err.kind(), std::io::ErrorKind::NotFound) {
                    return Ok(response::not_found());
                } else {
                    return Err(Error::new(err)).context("metadata for folder download");
                }
            }
        };
        if meta.is_file() {
            serve_file_from_fs(&full_path, None, None).await
        } else {
            let mut download_name = folder_path
                .file_name()
                .and_then(OsStr::to_str)
                .map(std::borrow::ToOwned::to_owned)
                .unwrap_or_else(|| "audio".into());

            download_name.push_str(format.extension());

            match blocking(move || {
                let allow_symlinks = get_config().allow_symlinks;
                if let Some(folder_re) = include_subfolders {
                    collection::list_dir_files_with_subdirs(
                        base_path,
                        &folder_path,
                        allow_symlinks,
                        folder_re,
                    )
                } else {
                    collection::list_dir_files_only(base_path, &folder_path, allow_symlinks)
                }
            })
            .await
            {
                Ok(Ok(folder)) => {
                    let total_len: u64 = match format {
                        DownloadFormat::Tar => {
                            let lens_iter = folder.iter().map(|i| i.2);
                            async_tar::calc_size(lens_iter)
                        }
                        DownloadFormat::Zip => {
                            let iter = folder
                                .iter()
                                .map(|&(ref path, ref name, len)| (path, name.as_str(), len));
                            async_zip::calc_size(iter).context("calc zip size")?
                        }
                    };

                    debug!("Total len of folder is {:?}", total_len);

                    let stream: Box<dyn Stream<Item = _> + Unpin + Send> = match format {
                        DownloadFormat::Tar => {
                            let files = folder.into_iter().map(|i| i.0);
                            Box::new(async_tar::TarStream::tar_iter(files))
                        }
                        DownloadFormat::Zip => {
                            let files = folder.into_iter().map(|i| (i.0, i.1));
                            let zipper = async_zip::Zipper::from_iter(files);
                            Box::new(zipper.zipped_stream())
                        }
                    };

                    let disposition = format!("attachment; filename=\"{}\"", download_name);
                    let builder = HyperResponse::builder()
                        .typed_header(ContentType::from(format.mime()))
                        .header(CONTENT_DISPOSITION, disposition.as_bytes())
                        .typed_header(ContentLength(total_len));
                    Ok(builder.body(Body::wrap_stream(stream)).unwrap())
                }
                Ok(Err(e)) => Err(Error::new(e).context("listing directory")),
                Err(e) => Err(Error::new(e).context("spawn blocking directory")),
            }
        }
    };

    Box::pin(f)
}
