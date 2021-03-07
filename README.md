Audioserve
==========

**!!PLEASE UPDATE TO v0.15.0 (or newer) DUE TO IMPORTANT SECURITY FIX!!**

[![Build Status](https://travis-ci.org/izderadicka/audioserve.svg?branch=master)](https://travis-ci.org/izderadicka/audioserve)

Simple personal server to serve audio files from directories. Intended primarily for audio books, but anything with decent directories structure will do. Focus here is on simplicity and minimalistic design.

Server is in Rust,  default web client (HTML5 + Javascript) is intended for modern browsers (latest Firefox or Chrome) and is integrated with the server. There is also [Android client](https://github.com/izderadicka/audioserve-android) and API for custom clients.

For some background and video demo check this article [Audioserve Audiobooks Server - Stupidly Simple or Simply Stupid?](http://zderadicka.eu/audioserve-audiobooks-server-stupidly-simple-or-simply-stupid)

If you will install audioserve and make it available on Internet do not [underestimate security](#security-best-practices).

Media Library
-------------

Audioserve is intended to serve files from directory in exactly same structure (recently with some support for .m4b and similar single file audiovooks), audio tags are not considered.  So recommended structure is:

    Author Last Name, First Name/Audio Book Name
    Author Last Name, First Name/Series Name/Audio Book Name

Files should be named so they are in right alphabetical order - ideal is:

    001 - First Chapter Name.opus
    002 - Second Chapter Name.opus

But this structure is not mandatory -  you will just see whatever directories and files you have, so use anything that will suite you.

The characters `$$` and `|`  are used for internal usage of audioserve, so you should not use then in file names.

In folders you can have additional metadata files - first available image (jpeg or png) is taken as a cover picture and first text file (html, txt, md) is taken as description of the folder.

Search is done for folder names only (not individual files, neither audio metadata tags).

You can have several libraries/ collections - just use several root directories as audioserve start parameters. In client you can switch between collections in the client. Typical usage will be to have separate collections for different languages.

By default symbolic(soft) links are not followed in the collections directory (because if incorrectly used it can have quite negative impact on search and browse), but they can be enabled by `--allow-symlinks` program argument.

### Single file audiobooks and chapters

Recently better support for .m4b (one big file with chapters metadata) and similar was added. Such file is presented as a folder, which contains chapters (if you do not like this feature you can disable with `--ignore-chapters-meta` argument). 

Also long audiofile without chapters metadata, can be split into equaly sized parts/chapters (this has a slight disadvantage as split can be in middle of word). To enable later use `--chapters-from-duration` to set a limit, from which it should be used, and `chapters-duration` to set a duration of a part. Also for large files, which do not have chapters metadata, you can easily supply them in a separate file, with same name as the audio file but with additional extension `.chapters` - so it looks like `your_audiobook.mp3.chapters`. This file is simple CSV file (with header), where first column is chapter title, second is chapter start time, third (and last) is the chapter end time.  Time is either in seconds (like `23.836`) or in `HH:MM:SS.mmm` format (like `02:35:23.386`).

There are some small glitches with this approach - search still works on directories only and cover and description metadata are yet not working (plan is to extract them from the audio file metadata). Apart of that chapters behaves like other audio files - can be transcoded to lower bitrates, seeked within etc.

If chaptered file is a single file in a directory (and there are no other subdirectories), then chapters are presented within this directory, as if they were files in this directory. This can help overcome above mentioned limitations - as search will work on directory name and also cover and description is shown from this directory - so this would be preferred way of placing .m4b files. If you do not like this new feature you can disable by `--no-dir-collaps` option.

Also beware that web client will often load same part of chapter again if you're seeking within it (especially Firefox with m4b), so it's definitely not bandwidth optimal (similar issue appears when often seeking in transcoded file).

Sharing playback positions between clients
-----------------------------------------

Recently (from version 0.10) audioserve supports sharing playback positions between clients. This is basically used to continue listening on next client, from where you left on previous one. It's supported in the included web client and in the recent Android client (from version 0.8). In order to enable position sharing you'll need to define 'device group' in the client (on login dialog in web client and in settings in Android client) - group is just an arbitrary name and devices within same group will share playback position.

After you have several active devices with same group name, you'll be notified when you click play and there is more recent playback position in the group and you can choose if jump to this latest position or continue with current position. There is also option to check latest position directly (in web client it's icon in the folder header, in Android client it's in options menu).

Proper functioning is (indeed) dependent on good connectivity -  as position is shared during playback via web socket connection. If connection is unstable this can be unreliable or behave bit strangely.

Security
--------

Audioserve is not writing anything to your media library, so read only access is OK. However you should assume that any file in publish media directories can be accessible via audioserve API (thought recently name starting with . (hidden files/directories) are blocked) so basically to anybody who can obtain shared secret (or in case you use `--no-authentication` then to anybody).

The only file where it needs to write is a file were it keeps its secret key for authentication (by default in `~/.audioserve/audioserve.secret`, but it can be specified by command line argument). And optionaly it writes files to transcoding cache ([see below](#transcoding-cache)) and positions file.

Authentication is done by shared secret phrase (supplied to server on command line or more securely via environment variable), which clients must know.
Shared secret phrase is never sent in plain (it's sent as salted hash). If correct shared secret hash is provided by client, sever generates a token, using its secret key, which is then used for individual requests authentication.  Token then can be used in cookie or HTTP Authorization header (Bearer method).
Token validity period is one year by default, but can be set with command line argument, but system generally expects token validity to be at least 10 days.
As the token can be used to steal the session, https is recommended (TLS support is build in, but reverse proxy is probably better solution). If you want to change shared secret also delete server secret (it will invalidate all issued tokens) - stop audioserve, delete `~/.audioserve/audioserve.secret` and restart audioserve with new shared secret.

Authentication is used to access all URLs except web client static files (`/index.html` and `/bundle.js`).

### TLS/SSL

Audioserve supports TLS/SSL - to enable it you need to provide your private server key as PKCS#12 file (in `--ssl-key` argument). Here is quick tip how to create private key with self-signed certificate (for testing purposed only):

    openssl req -newkey rsa:2048 -nodes -keyout key.pem -x509 -days 365 -out certificate.pem \
        -subj "/C=CZ/ST=Prague/L=Prague/O=Ivan/CN=audioserve"
    openssl pkcs12 -inkey key.pem -in certificate.pem -export  -passout pass:mypass -out audioserve.p12
    rm key.pem certificate.pem

You can also run audioserve behind reverse proxy like nginx or ha-proxy and terminate SSL there (in that case you can compile audioserve without TLS support see compilation without default features below)

#### Reverse proxy

Often best way how to deploy audioserve is behind reverse proxy, which terminates TLS/SSL and connects to backend audioserve. Reverse proxy can serve also other backend servers on same domain, in this case audioserve server should be determined by URL path prefix - so external address is like https://yourdomain.com/audioserve and it's map to http://local_name_or_ip:3000 (or whatever port you are using). Decent proxy can do such mapping, but I've heard about setups (shared seedbox), when this is not possible and URL path prefix is automatically forwarded to backend. For that case audioserve has argument `--url-path-prefix`, which contains prefix (without final slash) and audioserve accepts this prefix as root path.

Another gotcha for reverse proxy might be usage of last [playback position](#sharing-playback-positions-between-clients) feature, which requires websocket connection and some special configuration for that might be needed in reverse proxy.

Also there is optional feature `behind-proxy`, which enables argument `--behind-proxy`, is used only for logging real client ip address - if used client ip address is taken from `Forwarded` (preferred) or `X-Forwarded-For` HTTP headers provided by reverse proxy.

You can check some reverse proxy configurations in [reverse_proxy.md](./docs/reverse_proxy.md) (If you have successful configuration of reverse proxy please share via PR).

### Limit Requests Rate

Normally you'd allow audioserve to serve as much requests as it can handle, but if you like to protect yourself against DDoS (Distributed Denial of Service) attack (consider now much it's probable and serious in your case) you should consider limiting rate of requests handling. 

If audioserve is behind reverse proxy you can use rate limiting option of proxy server ([like this one for nginx](https://www.nginx.com/blog/rate-limiting-nginx/)).  Audioserve also has argument `--limit-rate n`, which turns on simple (it's global, not per remote address) rate limiting on all incoming HTTP requests to maximum of n request per second (approximately), for requests over the limit audioserve return 429 - Too Many Requests HTTP status code. As this is overall limit it will not protect legal users, as they will also see rejected requests, but it will just protect host from extensive use of resources.

Number of parallel transcodings (transcodings are most resource intensive tasks) is limited by `--transcoding-max-parallel-processes`, which is 2 * number of CPU cores by default. This is different then limit-rate, as it guards number of transcodings that run concurrently.

### Security Best Practices

- Always SSL/TLS - ideally behind well proven reverse proxy (I'm using nginx) (audioserve has support for SSL/TLS, but reverse proxy is probably more solid, plus can provide additional safeguards)
- Set solid shared secret 10+ different character types ... (to prevent guessing and brute force attacks), do not run on Internet with `no-authentication` - it's for testing only
- Never run audioserve as root
- in $HOME/.audoserve are couple of files that are writable by server - so they should have proper permissions - especially `audioserve.secret` should be be limited to user (running audioserve) access only
- Never put any secret information into media directories - all content of these directories is potentially accessible via Web API.
- Running in dedicated container also improves security
- if using remote proxy limit listening (`--listen` argument) interface of audioserve to one reachable by remote proxy only (for instance if they are on same server use `--listen 127.0.0.1:3000`)
- Optionally use your reverse proxy features like URL blocking, rate limiting etc. for additional security
- It's good to check logs from time to time - audioserve by default logs errors (including invalid access attempt) to stderr (which can be easily redirected to file), access log of reverse proxy is also useful
- Change shared secret (ideally in some larger (months) regular intervals, but it bit annoying), but for sure change in case you suspect it has been breached - always also delete server secret file.

Performance
-----------

Audioserve is intended to serve personal audio collections of moderate sizes. For sake of simplicity it does not provide any large scale performance optimizations.  It's fine to serve couple of users from collections of couple of thousands audiobooks, if they are reasonably organized. That's it, if you're looking for solution for thousands or millions of users, look elsewhere. To compensate for this audioserve is very lightweight and by itself takes minimum of system resources.

Browsing of collections is limited by speed of the file system. As directory listing needs to read audio files metadata (duration and bitrate, eventually chapters), folders with too many files (> 200) will be slow to open. Search is done by walking through collection directory structure, it can be slow - especially the first search (subsequent searches are much, much faster, as directory structure from previous search is cached by OS for some time). Recent audioserve versions provide optional search cache, to speed up search significantly - [see below](#search-cache).

But true limiting factor is transcoding - as it's quite CPU intensive. Normally you should run only a handful of transcodings in parallel, not much then 2x - 4x more then number of cores in the machine. For certain usage scenarios enabling of [transcoding cache](#transcoding-cache) can help a bit.

### Search Cache

For fast searches enable search cache with `--search-cache`, it will load directory structure of collections into memory, so searches will be blazingly fast (for price of more occupied memory). Search cache monitors directories and update itself upon changes (make take a while). Also after start of audioserve it takes some time before cache is filled (especially when large collections are used), so search might not work initially.

### Transcoding Cache

Optionally you can enable transcoding cache (by compiling audioserve with `transcoding-cache` feature). Contribution of this cache to overall performance depends very much on usage scenarios.  If there is only one user, which basically listens to audiobooks in linear order (chapter after chapter, not jumping back and forth), benefit will be minimal. If there are more users, listening to same audiobook (with same transcoding levels) and/or jumping often back and forth between chapters, then benefit of this cache can be significant. You should test to see the difference (when transcoding cache is compiled in it can be still disabled by `--t-cache-disable` option).

Transcoding
-----------

Audioserve offers possibility to transcode audio files to opus format (opus codec, ogg or webm container) or few other formats to save bandwidth and volume of transferred data. For transcoding to work `ffmpeg` program must be installed and available on system's PATH.
Transcoding is provided in three variants and client can choose between them (using query parameter `trans` with value l,m or h):

* low - (default 32 kbps opus with 12kHz cutoff mono)
* medium - (default 48 kbps opus with 12kHz cutoff)
* high - (default 64 kbps opus with 20kHz cutoff)

As already noted audioserve is intended primarily for audiobooks and believe me opus codec is excellent there even in low bitrates. However if you want to change parameters of these three transcodings you can easily do so by providing yaml confing file to argument `--config`. Here is sample file:

```yaml
transcoding:
    low:
    opus-in-ogg:
        bitrate: 16
        compression_level: 3
        cutoff: WideBand
        mono: true
    medium:
    opus-in-ogg:
        bitrate: 24
        compression_level: 6
        cutoff: SuperWideBand
        mono: true
    high:
    opus-in-ogg:
        bitrate: 32
        compression_level: 9
        cutoff: SuperWideBand
```

In each key first you have specification of codec-container combination, currently it supports `opus-in-ogg`, `opus-in-webm`, `mp3`, `aac-in-adts` (but other containers or codecs can relatively easily be added, provided they are supported by ffmpeg and container creation does not require seekable output - like MP4 container).

I have good experiences with `opus-in-ogg`, which is also default. `opus-in-webm` works well in browsers (and is supported  in browsers MSE API), but as it does not contain audio duration after trascoding, audio cannot be sought during playback in Android client, which is significant drawback. `mp3` is classical MPEG-2 Audio Layer III audio stream. `aac-in-adts` is AAC encoded audio in ADTS stream, it also may have problems with seeking in Android client.

For opus transcodings there are 3 other parameters, where `bitrate` is desired bitrate in kbps, `compression_level` is determining audio quality and speed of transcoding with values 1-10 ( 1 - worst quality, but fastest, 10 - best quality, but slowest ) and `cutoff` is determining audio freq. bandwidth (NarrowBand => 4kHz, MediumBand => 6kHz, WideBand => 8kHz, SuperWideBand => 12kHz, FullBand => 20kHz).

For mp3 transcoding there are also 3 parameters: `bitrate` (in kbps), `compression_level` with values 0-9 (0 - best quality, slowest, 9-worst quality, fastest; so meaning is inverse then for opus codec) and `abr` (optional), which can be `true` or `false` (ABR = average bit rate - enables ABR, which is similar to VBR, so it can improve quality on same bitrate, however can cause problems, when seeking in audion stream).

`aac-in-adts` has one mandatory parameter `bitrate` (in kbps) and two optional parameters `sr` - which is sample rate of transcoded stream (8kHz, 12kHz, 16kHz, 24kHz, 32kHz, 48kHz, unlimited) and `ltp` (Long Term Prediction), which is `true` or `false` and can improve audio quality, especially for lower bitrates, but for significant performance costs ( abou 10x slower).

All encodings have optional parameter `mono`, if set to `true` audio will be down-mixed to mono.

Overall `opus-in-ogg` provides best results from both quality and  functionality perspective, so I'd highly recommend to stick to it, unless you have some problem with it.

You can override one two or all three defaults, depending on what sections you have in this config file.

Command line
------------

Audioserve can take parameters from command line, environment variables and config file. For command line arguments check them with `audioserve -h`. Generally you need to provide shared secrect (or option `--no-authentication` for public access) and root of media library (as noted above you can have severals libraries).

`audioserve`  is server executable and it also needs web client files , which are `index.html` and `bundle.js`, which are defaultly in `./client/dist`, but their location can by specified by argument `-c`.

For majority of command line arguments there is also appropriate environment variable, which starts with prefix `AUDIOSERVE_` and then command line argument name (without leading dashes) transcribed from kebab-case to SCREAMING_SNAKE_CASE, so for instance argument `--shared-secret` has coresponding env. variable `AUDIOSERVE_SHARED_SECRET`.

All audioserve parameters can be also provided in configuration file via `--config` argument. Configuration file is in YAML format and somehow coresponds to command line arguments, but not exactly. Easiest way how to create config file is to use argument `--print-config`, which prints current configuration, including all used arguments to standard output.

Web client
----------

Web client is bundled with server. It provides simple interface (using bootstrap 4 CSS framework and JQuery JS library). Web client will remember your last playback position in a folder, so you can easily continue listening, even after page reload. Use three vertical dots in top right corner to choose required transcoding and subfolder items ordering.
Otherwise it's rather minimalistic (following KISS principle).

It's tested on Firefox and Chrome (on Linux and Android, should work on Windows, on OSX too on these browsers).
On iOS default transcoding (opus+ogg) is not working - so switch transcoding off or try custom transcoding profile.

Web client is not working on MS Edge(this might be fixed in future) and IE (which will never be supported).

Android client
--------------

Android client code is [available on github](https://github.com/izderadicka/audioserve-android).

API
---

audioserve server provides very simple API (see [api.md](./docs/api.md) for documentation), so it's easy to write your own clients.

Installation
------------

### Docker Image

Easiest way how to test audioserve (but do not use `--no-authentication` in production) is to run it as docker container with prebuild [Docker image](https://cloud.docker.com/u/izderadicka/repository/docker/izderadicka/audioserve) (from Docker Hub):

    docker run -d --name audioserve -p 3000:3000 -v /path/to/your/audiobooks:/audiobooks  izderadicka/audioserve --no-authentication /audiobooks

Then open <http://localhost:3000> - and browse your collection.  This is indeed the very minimal configuration of audioserve. For real deployment you'd like provide provide more command line parameters (or environment variables or your custom config file) - see more complex example below.

Of course you can build your own image very easily with provided `Dockerfile`, just run:

    docker build --tag audioserve .

When building docker image you can use `--build-arg CARGO_ARGS=` to modify cargo build command and to add/or remove features (see below for details). For instance this command will build audioserve with transcoding cache `docker build --tag audioserve --build-arg CARGO_ARGS="--features transcoding-cache" .`

There is also one additional env. variable `PORT` - TCP port on which audioserve serves http(s) requests (defaults to: 3000) - this is useful for services like Heroku, where container must accept PORT variable from the service.

A more detailed example (audioserve is an entry point to this container, so you can use all command line arguments of the program, to get help for the program run this command `docker run -it --rm  izderadicka/audioserve --help`):

    docker run -d --name audioserve -p 3000:3000 \
        -v /path/to/your/audiobooks1:/collection1 \
        -v /path/to/your/audiobooks2:/collection2 \
        -v /path/for/audioserve-data:/home/audioserve/.audioserve \
        -e AUDIOSERVE_SHARED_SECRET=mypass \
        izderadicka/audioserve \
        --ssl-key /audioserve/ssl/audioserve.p12 --ssl-key-password mypass \
        --search-cache \
        /collection1 /collection2

In the above example, we are adding two different collections of audiobooks (collection1 and collection2).
Both are made available to the container via `-v` option and then passed to audioserve on command line.
Also we have maped with `-v` some folder to `/home/audioserve/.audioserve`, where runtime data of audioserve are stored (server secret, caches ...)

We set the shared secret via `AUDIOSERVE_SHARED_SECRET` env.variable and specify use of TLS via `--ssl-key` and `ssl-key-password` (the tests only self-signed key is already prebundled in the image, for real use you'll need to generate your own key, or use reverse proxy that terminates TLS). We also enable search cache with `--search-cache` argument.

### Static build (Linux)

Static build of audioserve is available (for rencent releases) at [github releases page](https://github.com/izderadicka/audioserve/releases). You can can just download and extract locally and run on any modern x86_64 linux.
You can also create your own static build with script `build_static.sh` (Docker is required for this)

### Local build (Linux)

Now audioserve depends on ffmpeg's libavformat 4.3 (and its dependent libavutil and libavcodec libs), which is a complex beast. If you are building locally you need this dependence (plus couple of others). If you have available right version on your system you can dynamically link against it (remember it has to be correct version). Other option is to use feature `partially-static`, which will download right version of ffmpeg, compile it and statically link it into audioserve (but then binary will be indeed bigger).

Install required dependencies (some dependencies are optional, depending on features chosen in build):

    # Ubuntu - for other distros look for equivalent packages
    sudo apt-get install -y  openssl libssl-dev pkg-config\
        ffmpeg yasm build-essential wget libbz2-dev zlib1g-dev libavformat-dev

Clone repo with:

    git clone https://github.com/izderadicka/audioserve

To install locally you need recent [Rust](https://www.rust-lang.org/en-US/install.html) and [NodeJS](https://nodejs.org/en/download/package-manager/) installed.  

Compile Rust code (it has optional system dependencies to openssl,zlib, bz2lib, and  libavformat, as you'll not have exact correct version of libavformat around, it's better to build required version statically into binary with `partially-static` feature, beacause otherwise you might see problems like segfaults):

    cargo build --release --features partially-static

Optionaly you can compile with/without other features (see below for details).

Build client in its directory (`cd client`):

    npm install
    npm run build

### Windows build

**WARNING**: Windows are not officially supported - windows build instructions are contributed and resulting executable has some limitations (no features) and issues.

Clone the repository with:

    git clone https://github.com/izderadicka/audioserve

or download the ZIP archive. 

Download the `x86_64-pc-windows-msvc` version of Rust compiler from [rust-lang.org](https://forge.rust-lang.org/infra/other-installation-methods.html#standalone). Also download and perform the default installation of the [Build tools for Visual Studio](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=BuildTools&rel=16). You may ignore the request to reboot the system after the installation.

Download the 4.1 "development" version of Windows 64-bit binaries of FFmpeg from [the official website](https://ffmpeg.zeranoe.com/builds/win64/dev/). Extract all `/lib/*.lib` files such as `avcodec.lib` from the archive to the `C:\Program Files\Rust stable MSVC 1.43\lib\rustlib\x86_64-pc-windows-msvc\lib` folder.

Open a Command prompt or a PowerShell window, change to the directory where you have previously extracted the contents of `audioserve-master` and run:

    cargo build --release --no-default-features

After compilation, you will find the compiled binary, `audioserve.exe`, in the `target\release` sub-folder.

Next, switch to the `client` folder under `\target\release`. Install the NPM software from https://nodejs.org/en/download/ and build the client using: 

    npm install
    npm run build

Transfer the resulting `audioserve.exe` with the entire contents of the `client` folder to the preferred location.

#### Known issues:

 * compilation with the `--features partially-static` option does not work (instead, use the shared FFmpeg libraries as described above).
 * Audioserve doesn't recognize the paths that contain drive letters (i.e. `C:\`) and paths with symlinks or directory junctions. Put the `audioserve.exe` to the same disk drive as the folder with audio files and use it with paths _relative to the root_ of the drive. For example, if the path to the program is `d:\Audioserve\audioserve.exe`, its data folder is `D:\Audioserve\data` and your audio files are located in `C:\Audiobooks\`, launch the program as `D:\Audioserve\audioserve.exe --no-authentication --data-dir \Audioserve\data \Audiobooks`.
 * As the result of the above, you can not use the multiple folders with audio files across the different disks with Audioserve.
 * `Audioserve.exe` does not have an application icon.
 * The program keeps the terminal window open while it is running. To hide it, use any Windows [utility](https://robotronic.de/runasserviceen.html) that allows launching terminal programs as "Windows services" in the background.
 * Above instructions were only tested on a 64-bit Windows 10 platform.

### Other platforms

Theoretically audioserve can work on MacOS (probably with only  few changes in the code and building process (of libavformat)), but I never tried to build it there. Any help in this area is welcomed.

### Compiling without default features or with non-default features

TLS support (feature `tls`), symbolic links (feature `symlinks`) and search cache (feature `search-cache`) and folder download (feature `folder-download`) are default features, but you can compile without them - just add `--no-default-features` option to `cargo build` command. And then evetually choose only features you need.
To add non-default features (like `transcoding-cache`) compile with this option `--features transcoding-cache` in `cargo build` command.

**Available features:**

Feature | Description | Default | Program options
--------|------|:------:|---------------
| tls | Enables TLS support (e.g https) (requires openssl and libssl-dev) | Yes | --ssl-key --ssl-key-password to define server key
| symlinks | Enables to use symbolic links in media folders | Yes | Use --allow-symlinks to follow symbolic links
| search-cache | Caches structure of media directories for fast search | Yes | Use --search-cache to enable this cache
| folder-download | Enables API endpoint to download content of a folder in tar archive | Yes | Can be disabled with argument --disable-folder-download
| shared-positions | Clients can share recent playback positions via simple websocket API | Yes |
| transcoding-cache | Cache to save transcoded files for fast next use | No | Can be disabled by --t-cache-disable and modified by --t-cache-dir --t-cache-size --t-cache-max-files
| behind-proxy | Enable logging of client address from proxy headers | No | Enables argument --behind-proxy which should be use to log client address from headers provided by reverse proxy
| static | Enables fully static build of audioserve. Check above notes for static build | No |
| partially-static | Statically links libavformat (and related).Enables to run audioserve on systems, which do not have required version of libavformat| No |
| folder-download-default-tar | Default folder download format is tar (instead of zip) | No |

License
-------

[MIT](https://opensource.org/licenses/MIT)
