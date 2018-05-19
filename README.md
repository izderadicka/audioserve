Audioserve
==========
[![Build Status](https://travis-ci.org/izderadicka/audioserve.svg?branch=master)](https://travis-ci.org/izderadicka/audioserve)

Simple personal server to serve audio files from directories. Intended primarily for audio books, but anything with decent directories structure will do. Focus here is on simplicity and minimalistic design.

Server is in Rust,  default client is in Javascript intended for modern browsers (latest Firefox or Chrome) and is integrated with the server. There is also [Android client](https://github.com/izderadicka/audioserve-android) and API for custom clients. 

For some background and video demo check this article [Audioserve Audiobooks Server - Stupidly Simple or Simply Stupid?](http://zderadicka.eu/audioserve-audiobooks-server-stupidly-simple-or-simply-stupid)

Media Library
-------------

Audioserve is intended to serve files from directory in exactly same structure, no audio tags are considered.  So recommended structure is:

    Author Last Name, First Name/Audio Book Name
    Author Last Name, First Name/Series Name/Audio Book Name

Files should be named so they are in right alphabetical order - ideal is:

    001 - First Chapter Name.opus
    002 - Seconf Chapter Name.opus

But this structure is not mandatory -  you will just see whatever directories and files you have, so use anything that will suite you.

In folders you can have additional metadata files - first available image (jpeg or png) is taken as a coverage picture and first text file (html, txt, md) is taken as description of the folder.

Search is done for folder names only (not individual files, neither audio metadata tags).

You can have several libraries/ collections - just use several root directories as audioserve start parametes. In client you can switch between collections in the client. Typical usage will be to have separate collections for different languages.

Security
--------

Audioserve is not writing anything to your media library, so read only access is OK.  The only one file where it needs to write is a file were it keeps its secret key for authentication (by default in `~/.audioserve.secret`, but it can be specified by command line argument).

Authentication is done by shared secret phrase (supplied to server on command line), which client must know.  Secret phrase is never sent in plain (it's sent as salted hash). If correct shared secret hash is provided sever generates a token, using its secret key.  Token then can be used in cookie or HTTP Authorization header (Bearer method). 
Token validity period is one year by default, but can be set as command line argument, but system 
generaly expects token validity to be at least 10 days.
As the token can be used to steal the session https is recomended (TLS support is build in).

### TLS/SSL

Audioserve supports TLS/SSL - to enable it you need to provide your private server key as PKCS#12 file (in `--ssl-key` argument). Here is quick tip how to create private key with self-signed certificate:

    openssl req -newkey rsa:2048 -nodes -keyout key.pem -x509 -days 365 -out certificate.pem \
        -subj "/C=CZ/ST=Prague/L=Prague/O=Ivan/CN=audioserve"
    openssl pkcs12 -inkey key.pem -in certificate.pem -export  -passout pass:mypass -out audioserve.p12
    rm key.pem certificate.pem

You can also run behind reverse proxy like nginx or ha-proxy and perminate SSL there (in that case you can compile audioserve without TLS support see below)


Performance
-----------

Audioserve is inteded to serve personal audio collections of moderate sizes. For sake of simplicity it does not provide any large scale perfomance optimalizations.  It's fine to serve couple of users from collection of couple of thousands audiobooks, if they are reasonably organized. That's it, if you're looking for solution for thousands or millions of users, look elsewere. To compensate for this audioserve is very lightweight and by itself takes minimum of system resources.

Browsing of collections is limited by speed of the file system. As directory listing needs to read audio files metadata (duration and bitrate), folders with too many files (> 200) will be slow. Search is done by walking through collection, it can be slow - especially the first search (subsequent searches are much, much faster, as directory structure from previous search is cached by OS for some time).

But true limiting factor is transcoding - as it's quite CPU intensive. Normally you should run only a handful of transcodings in parallel, not much then 2x - 4x more then there is the number of cores in the machine. 


Transcoding
-----------

Audioserve offers possibility to transcode audio files to opus format (opus codec, ogg container) to save bandwidth and volume of transfered data. For transcoding to work `ffmpeg` program must be installed and available on system's PATH.
Transconding is provided in three variants and client can choose between then (using query parameter trans with value l,m or h):

* low - (default 32 kbps opus with 12kHz cutoff)
* medium - (default 48 kbps opus with 12kHz cutoff)
* high - (default 64 kbps opus with 20kHz cutoff)

As already noted audioserve is intended primarily for audiobooks and believe me opus codec is excellent there even in low bitrates. However if you want to change parameters of these three trancodings you can easily do so by providing yaml confing file to parameter `--transcoding-config`. Here is sample file:
```yaml
low:
  bitrate: 16
  compression_level: 3
  cutoff: WideBand
medium:
  bitrate: 24
  compression_level: 6
  cutoff: SuperWideBand
high:
  bitrate: 32
  compression_level: 9
  cutoff: SuperWideBand
```
Where bitrate is desired bitrate in kbps, compression_level is determining audio quality and speed of transcoding with values 1-10 ( 1 - worst quality, but fastest, 10 - best quality, but slowest ) and cutoff is determining audio freq. bandwith (NarrowBand => 4kHz, MediumBand => 6kHz, WideBand => 8kHz, SuperWideBand => 12kHz, FullBand => 20kHz).
You can overide one two or all three defaults, depending on what sections you have in this config file.

Command line
------------
Check with `audioserve -h`. Only two required arguments are shared secrect and root of media library (as noted above you can have severals libraries).
`audioserve`  is server executable and it also needs web client files , which are `index.html` and `bundle.js`, which are defaultly in `./client/dist`, but their location can by specified by argument `-c` 

Android client
--------------
Android client code is [available on github](https://github.com/izderadicka/audioserve-android)
Client is in early beta stage (I'm using it now to listen to my audiobooks).

API
---
audioserve server provides very simple API (see [api.md](./docs/api.md) for documentation), 
so it's easy to write your own clients.


Installation (Linux)
------------

Install required dependencies:

    # Ubuntu - for other distros look for equivalent packages
    sudo apt-get install -y  openssl libssl-dev libtag1-dev libtagc0-dev ffmpeg

Clone repo with: 

    git clone https://github.com/izderadicka/audioserve

To install locally you need [Rust](https://www.rust-lang.org/en-US/install.html) and [NodeJS](https://nodejs.org/en/download/package-manager/) installed - compile with `cargo build --release` (Rust code have system dependencies to openssl and taglib) and build client in its directory:

    npm install
    npm run build

But easiest way how to test audioserve is to run it as docker container with provided `Dockerfile`, just run:

    docker build --tag audioserve .
    docker run -d --name audioserve -p 3000:3000 -v /path/to/your/audiobooks:/audiobooks  audioserve  

Then open https://localhost:3000 and accept insecure connection, shared secret to enter in client is mypass

Other platforms - theoretically audioserve can work on Windows and MacOS (probably with few changes), 
but I never tried to build it there. Any help in this area is welcomed.

### Compiling without default features
TLS support is default features, but you can compile without it - just add `--no-default-features` option to cargo.


License
-------

[MIT](https://opensource.org/licenses/MIT) 
