#!/bin/bash

if [[ -n "$1" ]]; then
  cd "$1"
fi

set -e
wget https://www.ffmpeg.org/releases/ffmpeg-4.1.tar.xz
tar xvf ffmpeg-4.1.tar.xz 
rm ffmpeg-4.1.tar.xz
cd ffmpeg-4.1/
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
  --enable-pic

make


