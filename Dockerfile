FROM ubuntu:16.04
MAINTAINER Ivan <ivan@zderadicka.eu>

RUN apt -o Acquire::https::No-Cache=True -o Acquire::http::No-Cache=True update &&\
    apt-get update &&\
    apt-get install -y pkg-config openssl libssl-dev libtag1-dev libtagc0-dev curl ffmpeg &&\
    curl -sL https://deb.nodesource.com/setup_8.x | bash - &&\
    apt-get install -y nodejs &&\
    adduser audioserve


WORKDIR /home/audioserve

COPY . audioserve_src
VOLUME /audiobooks

RUN chown -R audioserve:audioserve audioserve_src &&\
    mkdir /audioserve &&\
    chown audioserve:audioserve /audioserve

VOLUME /audiobooks

USER audioserve

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y

RUN export PATH=/home/audioserve/.cargo/bin:$PATH &&\
    cd audioserve_src &&\
    cargo build --release &&\
    cargo test --release

RUN cd audioserve_src/client &&\
    npm install &&\
    npm run build

RUN cp audioserve_src/target/release/audioserve /audioserve &&\
    mkdir /audioserve/client &&\
    cp -r audioserve_src/client/dist /audioserve/client &&\
    rm -r audioserve_src &&\
    rm -r .cargo

WORKDIR /audioserve

RUN mkdir ssl &&\
    cd ssl &&\
    openssl req -newkey rsa:2048 -nodes -keyout key.pem -x509 -days 365 -out certificate.pem \
        -subj "/C=CZ/ST=Prague/L=Prague/O=Ivan/CN=audioserve" &&\
    openssl pkcs12 -inkey key.pem -in certificate.pem -export  -passout pass:mypass -out audioserve.p12 &&\
    rm key.pem certificate.pem

EXPOSE 3000

CMD ./audioserve -s mypass --ssl-key ./ssl/audioserve.p12 --ssl-key-password mypass /audiobooks

    



