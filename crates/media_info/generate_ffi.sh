#! /bin/bash
bindgen \
--no-doc-comments \
--whitelist-type AVFormatContext \
--whitelist-type AVDictionary \
--whitelist-type AVChapter \
--whitelist-type AVRational \
--whitelist-function av_dict_get \
--whitelist-function av_dict_count \
--whitelist-function av_log_set_level \
--whitelist-function av_register_all \
--whitelist-function avformat_version \
--whitelist-function avformat_alloc_context \
--whitelist-function avformat_open_input \
--whitelist-function avformat_find_stream_info \
--whitelist-function avformat_close_input \
--whitelist-function av_dump_format \
--whitelist-var AV_LOG_QUIET \
--whitelist-var AV_DICT_IGNORE_SUFFIX \
--whitelist-var AV_TIME_BASE \
wrapper.h -- -I ffmpeg-4.3.1 \
> src/ffi.rs