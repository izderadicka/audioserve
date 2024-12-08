# check=error=true

ARG CARGO_ARGS
ARG CARGO_RELEASE="release"

FROM alpine:3.19 AS build
LABEL maintainer="Ivan <ivan@zderadicka.eu>"

ARG CARGO_ARGS
ARG CARGO_RELEASE

RUN apk update &&\
    apk add git bash curl yasm build-base pkgconfig \
    wget libbz2 bzip2-dev zlib zlib-dev rust cargo ffmpeg-dev ffmpeg \
    clang clang-dev gawk ctags llvm-dev icu icu-libs icu-dev

COPY . /audioserve 
WORKDIR /audioserve

RUN if [[ -n "$CARGO_RELEASE" ]]; then CARGO_RELEASE="--$CARGO_RELEASE"; fi && \
    echo BUILDING: cargo build ${CARGO_RELEASE} ${CARGO_ARGS} && \
    cargo build ${CARGO_RELEASE} ${CARGO_ARGS} &&\
    cargo test ${CARGO_RELEASE} --all ${CARGO_ARGS}

FROM node:22-alpine AS client

RUN apk add git &&\
    git clone https://github.com/izderadicka/audioserve-web.git /audioserve_client &&\
    cd /audioserve_client &&\
    npm install &&\
    npm run build-sw &&\
    npm run build &&\
    ./compress_dist.sh

FROM alpine:3.19

ARG CARGO_ARGS
ARG CARGO_RELEASE

COPY --from=build /audioserve/target/${CARGO_RELEASE:-debug}/audioserve /audioserve/audioserve
COPY --from=client /audioserve_client/dist /audioserve/client/dist

RUN adduser -D -u 1000 audioserve &&\
    chown -R audioserve:audioserve /audioserve &&\
    apk --no-cache add libbz2 zlib ffmpeg && \
    if [[ "$CARGO_ARGS" =~ "collation" ]]; then apk --no-cache add icu-libs; fi

WORKDIR /audioserve
USER audioserve

ENV PORT=3000
ENV RUST_LOG=info

EXPOSE ${PORT}

ENTRYPOINT [ "./audioserve" ] 
