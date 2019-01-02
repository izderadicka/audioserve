FROM rust:slim AS build
MAINTAINER Ivan <ivan@zderadicka.eu>

ARG FEATURES

RUN apt-get update && \
    apt-get install -y \
    curl \
    libbz2-dev \
    libssl1.0-dev \
    libtag1-dev \
    libtagc0-dev \
    openssl \
    pkg-config \
    yasm && \
    mkdir ffmpeg-static && \
    curl -sL https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz | \
    tar xJv -C ffmpeg-static --strip-components=1 --wildcards "*/ffmpeg" && \
    mv /ffmpeg-static/ffmpeg /usr/bin/ffmpeg

WORKDIR /src
COPY Cargo.toml Cargo.lock /src/
# To cache this step, we only copy the toml file above
RUN cargo fetch

COPY . /src

RUN cargo build --release ${FEATURES} && \
    cargo test --release ${FEATURES}

FROM node:8-alpine as frontend

WORKDIR /src
COPY client /src

RUN npm install && \
    npm run build

FROM debian:stretch-slim

VOLUME /audiobooks
COPY --from=build /audioserve_src/target/release/audioserve /audioserve/audioserve
COPY --from=frontend /audioserve_src/client/dist /audioserve/client/dist
COPY --from=build /ffmpeg-static/ffmpeg /usr/bin

RUN adduser audioserve && \
    chown -R audioserve:audioserve /audioserve && \
    apt -o Acquire::https::No-Cache=True -o Acquire::http::No-Cache=True update && \
    apt-get install -y libssl1.1 libtag1v5 libtagc0 libbz2-1.0

WORKDIR /audioserve
USER audioserve
ENV PORT=3000

EXPOSE ${PORT}

ENTRYPOINT ["/audioserve/audioserve"]
CMD ["/audiobooks"]