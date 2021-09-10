#!/bin/bash
set -x -e
if [[ -n "$1" ]]; then
  cd "$1"
fi

if [[ -z "$FFMPEG_VERSION" ]]; then
  echo FFMPEG_VERSION environmet variable must be defined like FFMPEG_VERSION=ffmpeg-4.1.5 >&2
  exit 2
fi

wget https://www.ffmpeg.org/releases/$FFMPEG_VERSION.tar.xz
tar xvf $FFMPEG_VERSION.tar.xz 
rm $FFMPEG_VERSION.tar.xz
cd $FFMPEG_VERSION/
./configure \
 --disable-programs \
 --disable-swresample \
 --disable-swscale \
 --disable-postproc \
  --disable-avfilter \
  --disable-network  \
  --disable-dct  \
  --disable-dwt \
  --disable-lsp \
  --disable-lzo \
  --disable-mdct \
  --disable-rdft \
  --disable-fft  \
  --disable-faan \
  --disable-pixelutils \
  --disable-avdevice \
  --disable-encoders \
  --disable-decoders \
  --disable-doc \
  --disable-vdpau \
  --enable-pic

make


