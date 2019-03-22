FROM alpine:edge AS build
MAINTAINER Ivan <ivan@zderadicka.eu>

ARG CARGO_ARGS

RUN apk update &&\
    apk add git bash openssl openssl-dev taglib taglib-dev curl yasm build-base \
    wget libbz2 bzip2-dev  zlib zlib-dev rust cargo ffmpeg-dev ffmpeg

COPY . /audioserve 
WORKDIR /audioserve

RUN cargo build --release ${CARGO_ARGS} &&\
    cargo test --release ${CARGO_ARGS}

RUN mkdir /ssl &&\
    cd /ssl &&\
    openssl req -newkey rsa:2048 -nodes -keyout key.pem -x509 -days 365 -out certificate.pem \
        -subj "/C=CZ/ST=Prague/L=Prague/O=Ivan/CN=audioserve" &&\
    openssl pkcs12 -inkey key.pem -in certificate.pem -export  -passout pass:mypass -out audioserve.p12 

FROM node:10-alpine as client

COPY ./client /audioserve_client

RUN cd audioserve_client &&\
    npm install &&\
    npm run build

FROM alpine:edge

VOLUME /audiobooks
COPY --from=build /audioserve/target/release/audioserve /audioserve/audioserve
COPY --from=client /audioserve_client/dist /audioserve/client/dist
COPY --from=build /ssl/audioserve.p12 /audioserve/ssl/audioserve.p12

RUN adduser -D -u 1000 audioserve &&\
    chown -R audioserve:audioserve /audioserve &&\
    apk update &&\
    apk add libssl1.1 taglib \
    libbz2 zlib ffmpeg

WORKDIR /audioserve
USER audioserve

ENV PORT=3000

EXPOSE ${PORT}

ENTRYPOINT [ "./audioserve" ] 
CMD [ "--no-authentication", "/audiobooks" ]






