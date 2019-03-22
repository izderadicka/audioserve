FROM alpine:edge AS build
MAINTAINER Ivan <ivan@zderadicka.eu>
ENV CARGO_ARGS=""
ENV FEATURES=""

RUN apk update &&\
    apk add git bash curl yasm build-base openssl-dev taglib-dev\
    wget zlib zlib-dev libbz2 bzip2-dev ffmpeg-dev rust cargo npm &&\
    mkdir /src &&\
    mkdir /.cargo &&\
    chmod a+rw /.cargo &&\
    mkdir /.npm &&\
    chmod a+rw /.npm
 
WORKDIR /src
ENV RUSTFLAGS="-C target-feature=+crt-static"
CMD  ["./_build_static.sh"]

