//#[cfg(feature = "folder-download")]
use super::{
    icon::icon_response,
    transcode::{guess_format, AudioFilePath, ChosenTranscoding, QualityLevel, Transcoder},
    types::*,
    Counter,
};
use crate::{config::get_config, error::Error};
use collection::{
    audio_meta::is_audio, extract_cover, extract_description, parse_chapter_path, TimeSpan,
};
use futures::prelude::*;
use myhy::headers::{ContentLength, ContentType};
use myhy::response::{
    self,
    body::wrap_stream,
    data_response,
    file::{send_file_simple, serve_file_from_fs, ByteRange},
    not_found, not_found_cached, ResponseBuilderExt, ResponseResult,
};
use myhy::Response;

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
};
use tokio::task::spawn_blocking as blocking;

#[cfg(not(feature = "transcoding-cache"))]
async fn serve_file_cached_or_transcoded(
    full_path: PathBuf,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    _range: Option<ByteRange>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: ChosenTranscoding,
) -> ResponseResult {
    serve_file_transcoded_checked(
        AudioFilePath::Original(full_path),
        seek,
        span,
        transcoding,
        transcoding_quality,
    )
    .await
}

#[cfg(feature = "transcoding-cache")]
async fn serve_file_cached_or_transcoded(
    full_path: PathBuf,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    range: Option<ByteRange>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: ChosenTranscoding,
) -> ResponseResult {
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
    use myhy::response::file::serve_opened_file;

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

async fn serve_file_transcoded_checked(
    full_path: AudioFilePath<PathBuf>,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: ChosenTranscoding,
) -> ResponseResult {
    let counter = transcoding.transcodings;
    let running_transcodings = counter.load(Ordering::Acquire);
    if running_transcodings >= transcoding.max_transcodings {
        warn!(
            "Max transcodings reached {}/{}",
            running_transcodings, transcoding.max_transcodings
        );
        return Ok(response::too_many_requests());
    }

    counter.fetch_add(1, Ordering::Release);

    debug!(
        "Sendig file {:?} transcoded - remaining slots {}/{}",
        &full_path,
        transcoding.max_transcodings - running_transcodings - 1,
        transcoding.max_transcodings
    );
    serve_file_transcoded(full_path, seek, span, transcoding_quality, counter).await
}

async fn serve_file_transcoded(
    full_path: AudioFilePath<PathBuf>,
    seek: Option<f32>,
    span: Option<TimeSpan>,
    transcoding_quality: ChosenTranscoding,
    counter: Counter,
) -> ResponseResult {
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
        .transcode(full_path, seek, span, counter)
        .await
        .map(move |stream| {
            Response::builder()
                .typed_header(ContentType::from(mime))
                .header("X-Transcode", params.as_bytes())
                .body(wrap_stream(stream))
                .unwrap()
        })
}

pub async fn send_file<P: AsRef<Path>>(
    base_path: &'static Path,
    file_path: P,
    range: Option<ByteRange>,
    seek: Option<f32>,
    transcoding: super::TranscodingDetails,
    transcoding_quality: Option<ChosenTranscoding>,
) -> ResponseResult {
    let (real_path, span) = parse_chapter_path(file_path.as_ref());
    let full_path = base_path.join(real_path);
    if let Some(transcoding_quality) = transcoding_quality {
        debug!(
            "Sending file transcoded in quality {:?}",
            transcoding_quality.level
        );
        serve_file_cached_or_transcoded(
            full_path,
            seek,
            span,
            range,
            transcoding,
            transcoding_quality,
        )
        .await
    } else if span.is_some() {
        debug!("Sending part of file remuxed");
        serve_file_transcoded_checked(
            AudioFilePath::Original(full_path),
            seek,
            span,
            transcoding,
            ChosenTranscoding::passthough(),
        )
        .await
    } else {
        debug!("Sending file directly from fs");
        serve_file_from_fs(&full_path, range, None, false).await
    }
}

pub async fn send_description(
    base_path: &'static Path,
    file_path: impl AsRef<Path> + Send + 'static,
    cache: Option<u32>,
    can_compress: bool,
) -> ResponseResult {
    send_folder_metadata(
        base_path,
        file_path,
        "text/plain",
        cache,
        |p| extract_description(p).map(|s| s.into()),
        can_compress,
    )
    .await
}

pub async fn send_cover(
    base_path: &'static Path,
    file_path: impl AsRef<Path> + Send + 'static,
    cache: Option<u32>,
) -> ResponseResult {
    send_folder_metadata(
        base_path,
        file_path,
        "image/jpeg",
        cache,
        extract_cover,
        false,
    )
    .await
}

pub async fn send_folder_metadata(
    base_path: &'static Path,
    file_path: impl AsRef<Path>,
    mime: impl AsRef<str> + Send + 'static,
    cache: Option<u32>,
    extractor: impl FnOnce(PathBuf) -> Option<Vec<u8>> + Send + 'static,
    compressed: bool,
) -> ResponseResult {
    if is_audio(&file_path) {
        // extract description from audio file
        let full_path = base_path.join(file_path);
        let fut = blocking(move || {
            let last_modified = std::fs::metadata(&full_path)
                .and_then(|meta| meta.modified())
                .ok();
            let data = extractor(full_path);
            match data {
                None => not_found(),
                Some(data) => data_response(
                    data,
                    mime.as_ref().parse().unwrap(),
                    cache,
                    last_modified,
                    compressed,
                ),
            }
        })
        .map_err(Error::from);
        fut.await
    } else {
        send_file_simple(base_path, file_path, cache, compressed).await
    }
}

pub async fn send_folder_icon(
    collection: usize,
    folder_path: PathBuf,
    collections: Arc<collection::Collections>,
) -> ResponseResult {
    blocking(
        move || match collections.get_folder_cover_path(collection, folder_path) {
            Ok(Some((p, meta))) => icon_response(p, meta.into()),
            Ok(None) => Ok(not_found_cached(get_config().folder_file_cache_age)),
            Err(e) => {
                error!("error while getting folder icon: {}", e);
                Ok(not_found())
            }
        },
    )
    .await
    .map_err(Error::new)
    .and_then(|res| match res {
        Ok(x) => Ok(x),
        Err(e) => Err(e),
    })
}

#[cfg(feature = "folder-download")]
pub async fn download_folder(
    base_path: &'static Path,
    folder_path: PathBuf,
    format: DownloadFormat,
    include_subfolders: Option<regex::Regex>,
) -> ResponseResult {
    use anyhow::Context;
    use myhy::header::CONTENT_DISPOSITION;
    let full_path = base_path.join(&folder_path);
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
        serve_file_from_fs(&full_path, None, None, false).await
    } else {
        let mut download_name = folder_path
            .file_name()
            .and_then(OsStr::to_str)
            .map(std::borrow::ToOwned::to_owned)
            .unwrap_or_else(|| "audio".into());

        download_name.push_str(format.extension());

        let dir_listing = blocking(move || {
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
        .await;
        match dir_listing {
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

                let stream: Box<dyn Stream<Item = _> + Unpin + Send + Sync> = match format {
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
                let builder = Response::builder()
                    .typed_header(ContentType::from(format.mime()))
                    .header(CONTENT_DISPOSITION, disposition.as_bytes())
                    .typed_header(ContentLength(total_len));
                Ok(builder.body(wrap_stream(stream)).unwrap())
            }
            Ok(Err(e)) => Err(Error::new(e).context("listing directory")),
            Err(e) => Err(Error::new(e).context("spawn blocking directory")),
        }
    }
}
