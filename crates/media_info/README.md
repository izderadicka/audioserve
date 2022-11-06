# media_info

Simple Rust binding to libavformat (from ffmpeg project) to extract duration, bitrate, metadata and chapters from media file.

See [example code](examples/media_info.rs) for usage.

## requirements

Under Linux you'll need regular build environment (gcc, make, pkg-config ...) and nasm/yasm and zlib and bz2lib to build this crate.

Can also be statically linked with ffmpeg libraries - features partially-static or static (former statically links only ffmpeg libraries, later tries to link everything statically).

Static build process requires wget and access to Internet to get ffmpeg source.
