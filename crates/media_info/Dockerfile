FROM alpine:edge AS build
MAINTAINER Ivan <ivan@zderadicka.eu>

ARG FEATURES

RUN apk update &&\
    apk add git bash curl yasm build-base \
    wget zlib zlib-dev libbz2 bzip2-dev ffmpeg-dev rust cargo &&\
    adduser -D -u 1000 ivan &&\
    mkdir /src

USER ivan   
WORKDIR /src
ENV RUSTFLAGS="-C target-feature=+crt-static"
CMD  cargo build --target x86_64-alpine-linux-musl --release --example media_info --features static
   