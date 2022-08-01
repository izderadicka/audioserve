ARG CARGO_ARGS
ARG CARGO_RELEASE="release"
ARG NEW_CLIENT

FROM alpine:3.14 AS build
LABEL maintainer="Ivan <ivan@zderadicka.eu>"

ARG CARGO_ARGS
ARG CARGO_RELEASE

RUN apk update &&\
    apk add git bash openssl openssl-dev curl yasm build-base \
    wget libbz2 bzip2-dev  zlib zlib-dev rust cargo ffmpeg-dev ffmpeg \
    clang clang-dev gawk ctags llvm-dev icu icu-libs icu-dev

COPY . /audioserve 
WORKDIR /audioserve

RUN if [[ -n "$CARGO_RELEASE" ]]; then CARGO_RELEASE="--$CARGO_RELEASE"; fi && \
    echo BUILDING: cargo build ${CARGO_RELEASE} ${CARGO_ARGS} && \
    cargo build ${CARGO_RELEASE} ${CARGO_ARGS} &&\
    cargo test ${CARGO_RELEASE} --all ${CARGO_ARGS}

RUN mkdir /ssl &&\
    cd /ssl &&\
    openssl req -newkey rsa:2048 -nodes -keyout key.pem -x509 -days 365 -out certificate.pem \
        -subj "/C=CZ/ST=Prague/L=Prague/O=Ivan/CN=audioserve" &&\
    openssl pkcs12 -inkey key.pem -in certificate.pem -export  -passout pass:mypass -out audioserve.p12 


FROM node:14-alpine as client

ARG NEW_CLIENT

COPY ./client /audioserve_client

RUN if [[ -n "$NEW_CLIENT" ]]; then \
    echo "New client $NEW_CLIENT" && \
    rm -r  /audioserve_client/* &&\
    apk add git &&\
    git clone https://github.com/izderadicka/audioserve-web.git /audioserve_client &&\
    cd /audioserve_client &&\
    npm install &&\
    npm run build &&\
    npm run build-sw &&\
    mv public dist ;\
    else \
    echo "Old client" &&\
    cd audioserve_client &&\
    npm install &&\
    npm run build ;\
    fi

FROM alpine:3.14

ARG CARGO_ARGS
ARG CARGO_RELEASE

VOLUME /audiobooks
COPY --from=build /audioserve/target/${CARGO_RELEASE:-debug}/audioserve /audioserve/audioserve
COPY --from=client /audioserve_client/dist /audioserve/client/dist
COPY --from=build /ssl/audioserve.p12 /audioserve/ssl/audioserve.p12

RUN adduser -D -u 1000 audioserve &&\
    chown -R audioserve:audioserve /audioserve &&\
    apk --no-cache add libssl1.1 libbz2 zlib ffmpeg && \
    if [[ "$CARGO_ARGS" =~ "collation" ]]; then apk --no-cache add icu-libs; fi

WORKDIR /audioserve
USER audioserve

ENV PORT=3000
ENV RUST_LOG=info

EXPOSE ${PORT}

ENTRYPOINT [ "./audioserve" ] 
