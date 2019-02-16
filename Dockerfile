FROM debian:stretch-slim AS build
MAINTAINER Ivan <ivan@zderadicka.eu>

ARG FEATURES

RUN apt -o Acquire::https::No-Cache=True -o Acquire::http::No-Cache=True update &&\
    apt-get install -y git pkg-config openssl libssl-dev libtag1-dev libtagc0-dev curl yasm build-essential wget libbz2-dev zlib1g-dev &&\
    curl -sL https://deb.nodesource.com/setup_8.x | bash - &&\
    apt-get install -y nodejs 

COPY . /audioserve_src

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN mkdir ffmpeg-static &&\
    curl -sL https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz | tar xJv -C ffmpeg-static --strip-components=1 --wildcards "*/ffmpeg" &&\
    cp /ffmpeg-static/ffmpeg /usr/bin


RUN export PATH=${HOME}/.cargo/bin:$PATH &&\
    cd audioserve_src &&\
    cargo build --release ${FEATURES} &&\
    cargo test --release ${FEATURES}

RUN cd audioserve_src/client &&\
    npm install &&\
    npm run build

RUN mkdir /ssl &&\
    cd /ssl &&\
    openssl req -newkey rsa:2048 -nodes -keyout key.pem -x509 -days 365 -out certificate.pem \
        -subj "/C=CZ/ST=Prague/L=Prague/O=Ivan/CN=audioserve" &&\
    openssl pkcs12 -inkey key.pem -in certificate.pem -export  -passout pass:mypass -out audioserve.p12 

FROM debian:stretch-slim

VOLUME /audiobooks
COPY --from=build /audioserve_src/target/release/audioserve /audioserve/audioserve
COPY --from=build /audioserve_src/client/dist /audioserve/client/dist
COPY --from=build /ssl/audioserve.p12 /audioserve/ssl/audioserve.p12
COPY --from=build /ffmpeg-static/ffmpeg /usr/bin

RUN adduser audioserve &&\
    chown -R audioserve:audioserve /audioserve &&\
    apt -o Acquire::https::No-Cache=True -o Acquire::http::No-Cache=True update &&\
    apt-get install -y libssl1.1 libtag1v5 libtagc0 libbz2-1.0
   
WORKDIR /audioserve
USER audioserve

ENV PORT=3000

EXPOSE ${PORT}

ENTRYPOINT [ "./audioserve" ] 
CMD [ "--no-authentication", "/audiobooks" ]
