#! /bin/bash

if [[ -z "$FFMPEG_VERSION" ]]; then
  echo FFMPEG_VERSION environmet variable must be defined like FFMPEG_VERSION=ffmpeg-4.1.5 >&2
  exit 2
fi

bindgen \
--no-doc-comments \
--allowlist-type AVFormatContext \
--allowlist-type AVDictionary \
--allowlist-type AVChapter \
--allowlist-type AVRational \
--allowlist-function av_dict_get \
--allowlist-function av_dict_count \
--allowlist-function av_log_set_level \
--allowlist-function av_register_all \
--allowlist-function avformat_version \
--allowlist-function avformat_alloc_context \
--allowlist-function avformat_open_input \
--allowlist-function avformat_find_stream_info \
--allowlist-function avformat_close_input \
--allowlist-function av_dump_format \
--allowlist-var AV_LOG_QUIET \
--allowlist-var AV_DICT_IGNORE_SUFFIX \
--allowlist-var AV_TIME_BASE \
wrapper.h -- -I $FFMPEG_VERSION \
> src/ffi_$FFMPEG_VERSION.rs