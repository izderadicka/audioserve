# Audioserve

[![Build](https://github.com/izderadicka/audioserve/actions/workflows/rust_check.yml/badge.svg)](https://github.com/izderadicka/audioserve/actions/workflows/rust_check.yml)
[![Docker Pulls](https://img.shields.io/docker/pulls/izderadicka/audioserve)](https://hub.docker.com/repository/docker/izderadicka/audioserve)

[ [**DEMO AVAILABLE** - shared secret: mypass] ](https://audioserve.zderadicka.eu)

**New web client is coming - it'll be now default for master branch and soon it'll be also in stable image release. New client is in  [separate project](https://github.com/izderadicka/audioserve-web)**

Simple personal server to serve audio files from directories. Intended primarily for audio books, but anything with decent directories structure will do. Focus here is on simplicity and minimalist design.

Server is written in Rust, new web PWA client (Typescript and Svelte) is focused on modern browsers and is using rather recent functionality (Service Worker), older, less demanding web client is still around - check [Web client chapter](#web-client) for details. There is also [Android client](https://github.com/izderadicka/audioserve-android) and [simple API](docs/api.md) for custom clients.

For some (now bit outdated) background and video demo check this article(bit old but gives main motivation behind it) [Audioserve Audiobooks Server - Stupidly Simple or Simply Stupid?](http://zderadicka.eu/audioserve-audiobooks-server-stupidly-simple-or-simply-stupid)

If you will install audioserve and make it available on Internet do not [underestimate security](#security-best-practices).

**Apple** users need to add additional configuration - [read this](#alternative-transcodings-and-transcoding-configuration-for-apple-users).

Like audioserve and want to start quickly and easily and securely? Try [this simple guide](docs/deploy.md) to have audioserve up and running for free in no time.

## TOC

- [Audioserve](#audioserve)
  - [TOC](#toc)
  - [Media Library](#media-library)
    - [Collections cache](#collections-cache)
    - [Single file audiobooks and their chapters](#single-file-audiobooks-and-their-chapters)
    - [Merge/collapsing of CD subfolders](#mergecollapsing-of-cd-subfolders)
    - [Audio files metadata tags](#audio-files-metadata-tags)
    - [Collation](#collation)
  - [Sharing playback positions between clients](#sharing-playback-positions-between-clients)
  - [Security](#security)
    - [TLS/SSL](#tlsssl)
      - [Reverse proxy](#reverse-proxy)
    - [Limit Requests Rate](#limit-requests-rate)
    - [CORS](#cors)
    - [Security Best Practices](#security-best-practices)
  - [Performance](#performance)
    - [Transcoding Cache](#transcoding-cache)
  - [Transcoding](#transcoding)
    - [Alternative transcodings and transcoding configuration for Apple users](#alternative-transcodings-and-transcoding-configuration-for-apple-users)
  - [Command line](#command-line)
  - [Web client](#web-client)
  - [Android client](#android-client)
  - [API](#api)
  - [Installation](#installation)
    - [Docker Image](#docker-image)
      - [Running audioserve in Docker as different user](#running-audioserve-in-docker-as-different-user)
      - [Docker Compose Example](#docker-compose-example)
    - [Static build (Linux)](#static-build-linux)
    - [Local build (Linux)](#local-build-linux)
    - [Other platforms](#other-platforms)
    - [Compiling without default features or with non-default features](#compiling-without-default-features-or-with-non-default-features)
  - [License](#license)

## Media Library

Audioserve is intended to serve files from directories in exactly same structure (with support for .m4b and similar single file audiobooks, where chapters are presented as virtual files), audio tags are not used for browsing, only optionally they can be displayed. Recommended directory structure of collections is:

    Author Last Name, First Name/Audio Book Name
    Author Last Name, First Name/Series Name/Audio Book Name

Files should be named so they are in right alphabetical order - ideally prefixed with padded number:

    001 - First Chapter Name.opus
    002 - Second Chapter Name.opus

But this structure is not mandatory - you will just see whatever directories and files you have, so use anything that will suite you.

Audioserve assumes that file and folder names are in UTF-8 encoding (or compatible), for other encodings it may not work correctly.

The characters `$$` and `|` are used for internal usage of audioserve, so you should not use them in file names.

In folders you can have additional metadata files - first available image (jpeg or png) is taken as a cover picture and first text file (html, txt, md) is taken as description of the folder.

Search is done for folder names only (not individual files, neither audio metadata tags).

You can have several collections/libraries - just use several collection directories as audioserve command arguments. In client you can switch between collections. Typical usage will be to have separate collections for different languages.

By default symbolic(soft) links are not followed in the collections directory (because if incorrectly used it can have quite negative impact on search and browse), but they can be enabled by `--allow-symlinks` program argument.

### Collections cache

Initially I though that everything can be just served from the file system. However experience with the program and users feedback have revealed two major problems with this approach:

- for larger collections search was slow
- a folder with many audiofiles (over couple hundreds) was loading slowly (because we have to collect basic audio metadata - duration and bitrate for each file)

So I implemented caching of collection data into embedded key-value database (using [sled](https://github.com/spacejam/sled)). I'm quite happy with it now, it really makes audioserve **superfast**.

However it brings bit more complexity into the program. Here are main things to consider:

- On start audioserve scans and caches collection directories. If it is first scan it can take quite some time (depending on size of collection, can be tens of minutes for larger collections). Until scan is complete search might not work reliably. Also on running audioserve you can enforce full collections rescan by sending signal `sigusr1` to the program.
- Content of the cache is influenced by several program arguments, namely `--tags`, `--tags-custom`, `--ignore-chapters-meta`, `--no-dir-collaps`, `--allow-sym-link`, `-chapters-duration`, `--chapters-from-duration`.   If audioserve is restarted and some of these arguments is changed, it may not be reflected in the cache (as cache is updated only when mtime of some file/directory changes and updates are local to directory changed).  In this case you'll need to force full reload of cache manually - either by sending `sigusr1` signal to program, or starting with `--force-cache-update` argument, which enforces full cache reload on start.
- audioserve is watching for collection directories changes (using inotify on linux) so if you change something in collection - add, change, rename, delete folders/files - changes will propagate to running audioserve automatically - you will just need to wait a small amount of time - like 15 secs, before changes are visible in the program. For large collections you should increase the limit of inotify watchers in linux:

```shell
cat /proc/sys/fs/inotify/max_user_watches
# use whatever higher number matches your requirement
echo fs.inotify.max_user_watches=1048576 | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

- cache is indeed bound with collection directory (hash of absolute normalized path is used as an identification for related cache) - so if you change collection directory path cache will also change (and old cache will still hang there - so some manual clean up might be needed).
- if you do not want to cache particular collection you can add `:no-cache` option after collection directory argument. However then position sharing and metadata tags will also not work for that collection and search will be slow.

### Single file audiobooks and their chapters

audioserve also supports single file audiobooks like .m4b (one big file with chapters metadata) and similar (.mp3 can also contain chapters metadata). Such file is presented as a folder (with name of original file), which contains chapters as "virtual" files (chapters behaves like other audio files - can be transcoded to lower bitrates, seeked within etc.) (if you do not like this feature you can disable with `--ignore-chapters-meta` argument, I have seen some .mp3 files, which contained bad chapters metadata).

Also long audiofile without chapters metadata, can be split into equaly sized parts/chapters (this has a slight disadvantage as split can be in middle of word). To enable this use `--chapters-from-duration` to set a limit, from which it should be used, and `chapters-duration` to set a duration of a part. Also for large files, which do not have chapters metadata, you can easily supply them in a separate file, with same name as the audio file but with additional extension `.chapters` - so it looks like `your_audiobook.mp3.chapters`. This file is simple CSV file (with header), where first column is chapter title, second is chapter start time, third (and last) is the chapter end time. Time is either in seconds (like `23.836`) or in `HH:MM:SS.mmm` format (like `02:35:23.386`).

If chaptered file is a single file in a directory (and there are no other subdirectories), then chapters are presented within this directory, as if they were files in this directory and cover and description is shown from this directory. If you do not like this feature you can disable by `--no-dir-collaps` option.

Also note that web client will often load same part of chapter again if you're seeking within it (especially Firefox with m4b), so it's definitely not bandwidth optimal (similar issue appears when often seeking in transcoded file).

### Merge/collapsing of CD subfolders

Sometimes (mainly for historical reasons) content of audiobook is divided in CD subfolders, reflecting how it was originally distributed on physical media. In audioserve you have option to collapse all these CD subfolders into main root folder and thus see whole audiobook at once. File names then will be prefixed with CD subfolder name.
This is an optional feature and could be enabled by argument `--collapse-cd-folders` (will require full reload of collection cache) and will collapse CD subfolders if:

Only folders matching regular expression `r"^CD[ -_]?\s*\d+\s*$"` (case insensitive) are collapsed into parent folder. Custom regular expression can be provided by argument `--cd-folder-regexp`. 

### Audio files metadata tags

audioserve is using directory structure for navigation and searching. This is one of key design decisions and it will not change. Main reason is because tags are just one big mess for audiobooks, everybody uses them in slightly different way, so they are not reliable. This was key reason why I started work on audioserve - to see my collection is the same way in which I stored it on disk. I do not want to bother with tags cleanup.

However with new collection cache there is possibility to scan tags and display them in web client (not yet in Android). Just for display purpose, no special functionality is related to tags.

It's optional you'll need to start audioserve with `--tags` or `--tags-custom` (here you list tags you're interested in - use `--help-tags` for list of supported tags).

This is the algorithm for scanning tags: tags are scanned for all files in the folder, if particular tag (like artist) is same for all files in the folder it is put on folder level, otherwise is stays with the file. For chapterized big audiofile its tags are put on folder level (virtual folder representing this file). Chapter tags are not collected (in all .m4b I saw only tag was title, which is anyhow used for chapter virtual file name).

It assumed that tags are in UTF-8 encoding, if not incorrect character is replaced by unicode replacement char. Optionally you can compile audioserve with feature `tags-encoding`, which will enable argument of same name - here you can provide alternate character encoding that will be used if UTF-8 decoding fails. 

### Collation

By default audioserve alphabetic order of audio files and subfolders is case insensitive "C like" collation, meaning national characters like "č" are sorted after all ASCII characters and not after "c". For more advanced collation respecting local collation additional unicode support is needed. Unfortunately Rust does not have native support for this and only working library is binding to ICU C libraries, which makes compilation bit complicated. To support local/national collation audioserve has to be compiled with optional feature `collation`. Such version of audioserve will then use following env.variables to determine locale for collation (in order of precedence): `AUDIOSERVE_COLLATE`, `LC_ALL`, `LC_COLLATE`, `LANG`. If nothing is found it falls back to `en_US`, which still handles somehow national characters ("č" is equal to "c" in sorting).

## Sharing playback positions between clients

Audioserve supports sharing playback positions between clients. This is basically used to continue listening on next client, from where you left audio file on previous one. It's supported in the included web client and in the recent Android client (from version 0.8). In order to enable position sharing you'll need to define 'device group' in the client (on login dialog in web client and in settings in Android client) - group is just an arbitrary name and devices within same group will share playback position. This is **not user**, as there is no such concept in audioserve, it is just arbitrary identifier you set on several devices and they then share the playback position.

After you have several active devices with same group name, you'll be notified when you click play button and there is more recent playback position in the group and you can choose if to jump to this latest position or continue with current position. There is also option to check latest position directly (in web client it's icon in the folder header (shows something only if there if newer position then current), in Android client it's in options menu).

Proper functioning is (indeed) dependent on good connectivity - as position is shared during playback via web socket connection. If connection is unstable this can be unreliable or behave bit strangely.

Position tracking is tightly connected with collection cache, so it'll not work for collection, which do not use caching (specified with `:no-cache` option). You can also backup positions to JSON file (highly recommended) for restoration in case of disk problems or for migration of audioserve - check `--positions-backup-file` and `--positions-backup-schedule` arguments of the program. Also if former argument is present you can force immediate backup by sending signal `sigusr2` to the program.

Shared playback positions are behind default program feature `shared-positions`, so you can compile program without it.

## Security

Audioserve is not writing anything to your media library, so read only access is enough. However you should assume that any file in published media directories can be accessible via audioserve API (names starting with . (hidden files/directories) are blocked in API) to anybody who can obtain shared secret (or in case you use `--no-authentication` then to anybody).

Read and **write** access is needed to data directory (`~/.audioserve` by default, but can be changed with `--data-dir` argument). This directory contains:

- **server secret** - file were it keeps server secret key for authentication (by default `audioserve.secret`, but its locations can be changed by command line argument `--secret-file`) - this file should have exclusive rw access for user running audioserve (this is how file is created, so no special action is needed).
- **collections cache and playback positions** - are stored in key value database, separate database is created for each collection (by default in `col_db` subdirectory). Collection cache database name consists of last segment of collection path and hash of absolute normalized collection path.
- **transcoding cache** - optionally, if feature `transcoding-cache` ([see below](#transcoding-cache)) is enabled (during compilation) cache directory (by default in `audioserve-cache`, can be changed by argument `--t-cache-dir`), where already transcoded audio files are stored for later reuse.

Authentication is done by shared secret phrase (supplied to server on command line or more securely via environment variable), which users must know. Audioserve does not have any notion of explicit named users, shared secret is all that is needed to access it (as explained above it's designed for hosting personal audio collection for one user, or group of users who trust each other fully).

Shared secret phrase is never sent in plain (it's sent as salted hash). If correct shared secret hash is provided by client, sever generates a token, using its secret key, which is then used for individual requests authentication. Token then can be used in cookie or HTTP Authorization header (Bearer method).

Token validity period is one year by default, but can be set via command line argument, but generally it is expected that token validity is in order of weeks (clients are not designed for frequent token changes).

As the token can be used to steal the session, https is recommended (TLS support is build in, but reverse proxy is probably better solution). If you want to change shared secret also delete server secret (it will invalidate all issued tokens) - stop audioserve, delete `~/.audioserve/audioserve.secret` and restart audioserve with new shared secret.

Authentication is used to access all URLs except web client static files (`/index.html`, `/bundle.js` and similar).

### TLS/SSL

Audioserve supports TLS/SSL - to enable it you need to provide your private server key and it's corresponding certificates chain both in PEM format (this changed recently in version 0.20 as `rustls` is now  used, previously key and certificate were in single PKCS#12 file, I think PEM is more supported and easier to handle - it's similar how apache, nginx, etc. work, also with this change private key is no longer encrypted. Key and certificate are provided  in `--ssl-key` and `ssl-cert` arguments respectively. Here is quick tip how to create private key with self-signed certificate (for testing purposed only):

    openssl req -newkey rsa:2048 -nodes -keyout key.pem -x509 -days 365 -out certificate.pem \
        -subj "/C=CZ/ST=Prague/L=Prague/O=Ivan/CN=audioserve"


You can also run audioserve behind reverse proxy like nginx or ha-proxy and terminate SSL there (in that case you can compile audioserve without TLS support see compilation without default features below)

#### Reverse proxy

Often best way how to deploy audioserve is behind reverse proxy, which terminates TLS/SSL and connects to backend audioserve. Reverse proxy can serve also other backend servers on same domain, in this case audioserve server could be determined either by subdomain ( https://audioserve.yourdomain.com), which assumes that you can modify DNS records, or by URL path prefix - external address is like https://yourdomain.com/audioserve and it's map to http://local_name_or_ip:3000 backend host. Decent proxy can do such mapping using URL rewriting (removing path prefix), but in some setups (shared seedbox), it is not possible and URL path prefix is automatically forwarded to backend. For that case audioserve has argument `--url-path-prefix`, which can contain path prefix (without final slash) and audioserve accepts this prefix as root path.

Another gotcha for reverse proxy might be usage of last [playback position](#sharing-playback-positions-between-clients) feature, which requires websocket connection and some special configuration for that might be needed in reverse proxy.

Also there is optional feature `behind-proxy`, which enables argument `--behind-proxy` and is used for logging real client ip address - client ip address is taken from `Forwarded` (preferred) or `X-Forwarded-For` HTTP headers provided by reverse proxy.

You can check some reverse proxy configurations in [reverse_proxy.md](./docs/reverse_proxy.md) (If you have successful configuration of reverse proxy please share via PR).

### Limit Requests Rate

Normally you'd allow audioserve to serve as much requests as it can handle, but if you'd like to protect yourself against DDoS (Distributed Denial of Service) attack (consider how much probable and serious is this threat in your case), you should consider limiting rate of requests handling.

If audioserve is behind reverse proxy you can use rate limiting option of proxy server ([like this one for nginx](https://www.nginx.com/blog/rate-limiting-nginx/)). Audioserve also has argument `--limit-rate n`, which turns on simple (it's global, not per remote address) rate limiting on all incoming HTTP requests to maximum of n request per second (approximately), for requests over this limit audioserve returns 429 - Too Many Requests HTTP status code. As this is overall limit it will not protect legal users, as they will also see rejected requests, but it will just protect host from extensive use of resources.

Number of parallel transcodings (transcodings are most resource intensive tasks) is limited by `--transcoding-max-parallel-processes`, which is 2 \* number of CPU cores by default. This is different then limit-rate, as it guards only number of transcodings that run concurrently.

### CORS

When web client is served from different host (or port) then audioserve API then browser enforces [Cross-Origin Resource Sharing (CORS) rules](https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS). Basically it means that browser might refuse to connect to server, if server is not configured to send special HTTP headers.

Audioserve is handling this with with optional `--cors` argument, which will add appropriate CORS headers to responses, thus enabling browser to accept responses from server.  If you want to limit origins for which audioserve sends CORS header you can use additional argument `--cors-regex`, which will first check `Origin` request header against given regular expresssion, if matches only then appropriate CORS responses are sent (but if you want it to use think carefully about regex - regex is not always giving what you wish for:-)  

CORS is relevant in several scenarios:

- during web client development, when client is served from development server (for instance `webpack serve`) on one port, say 8080, and API is served from audioserve listening on other port, say 3000 browser CORS policies will then prevent client from communicating with audioserve server API (as they are on different posts, thus different origins), unless CORS headers are included in server responses.
- If you are using third party client (like [audiosilo](https://github.com/KodeStar/audiosilo)), client may sit in one domain, say https://client.audiosilo.app/, and audioserve in other domain, say https://audioserve.zderadicka.eu, so again here CORS headers are required (`--cors` argument when starting audioserve). Also in this case connection **must be secure** - https://.
- Audioserve responses' CORS headers are permissive by default, allowing access from all origins and with any additional headers enable any possible scenario of usage. You can limit CORS origins by regular expression by using `--cors-regex` argument instead - it will allow only origins matching given regular expression.

It is important to understand that CORS is not security measure for server, but for browser only. No matter if `--cors` is added or not server will accept correct (properly formatted and with valid token) requests from any client.

### Security Best Practices

- Always use SSL/TLS - ideally behind well proven reverse proxy (I'm using nginx) (audioserve has support for SSL/TLS, but reverse proxy is probably more solid, plus can provide additional safeguards)
- Set solid shared secret 14+ characters (to prevent guessing and brute force attacks), do not run on Internet with `no-authentication` - it's for testing only
- Never run audioserve as root
- Keep audioserve updated - as I'm addressing found issues in new releases (definitely do not use versions older then v0.15.0, which addressed very important security fix)
- in $HOME/.audoserve there are files, which are writable by server - so they should have proper permissions - especially `audioserve.secret` should be be limited to user (running audioserve) access only
- Never put any secret information into media directories - all content of these directories is potentially accessible via Web API.
- Running in dedicated container also improves security
- if using remote proxy limit audioserve listening address (`--listen` argument) to one reachable by remote proxy only (for instance if they are on same host use `--listen 127.0.0.1:3000`)
- Optionally use your reverse proxy features like URL blocking, rate limiting etc. for additional security
- It's good to check logs from time to time - audioserve by default logs errors (including invalid access attempts) to stderr (which can be easily redirected to file), access log of reverse proxy is also useful source of information
- Change shared secret (ideally in some larger (months) regular intervals), but for sure change it in case you suspect it has been breached - always also delete server secret file, when changing shared secret (it will invalidate existing tokens).

## Performance

Audioserve is intended to serve personal audio collections of moderate sizes. For sake of simplicity it does not provide any large scale performance optimizations. It's fine to serve couple of users from collections of couple of thousands audiobooks, if they are reasonably organized. That's it, if you're looking for solution for thousands or millions of users, look elsewhere. To compensate for this audioserve is very lightweight and by itself takes minimum of system resources.

To compensate for some slow file system operations audioserve by default is using collections cache system - see also [Collections Cache](#collections-cache) above.

If transcoding is used it is another significant limiting factor - as it's quite CPU intensive. Normally you should run only a handful of transcodings in parallel, not much then 2x - 4x more then number of cores in the machine. For certain usage scenarios enabling of [transcoding cache](#transcoding-cache) can help a bit.

### Transcoding Cache

Optionally you can enable transcoding cache (by compiling audioserve with `transcoding-cache` feature). Contribution of this cache to overall performance depends very much on usage scenarios. If there is only one user, which basically listens to audiobooks in linear order (chapter after chapter, not jumping back and forth), benefit will be minimal. If there are more users, listening to same audiobook (with same transcoding levels) and/or jumping often back and forth between chapters, then benefits of this cache can be significant. You should test to see the difference (when transcoding cache is compiled in it can be still disabled by `--t-cache-disable` option).

## Transcoding

Audioserve offers possibility to transcode audio files to opus format (opus codec, ogg or webm container) or few other formats to save bandwidth and volume of transferred data. For transcoding to work `ffmpeg` program must be installed and available on system's PATH.
Transcoding is provided in three variants and client can choose between them (using query parameter `trans` with value l,m or h):

- low - (default 32 kbps opus with 12kHz cutoff mono)
- medium - (default 48 kbps opus with 12kHz cutoff)
- high - (default 64 kbps opus with 20kHz cutoff)

As already noted audioserve is intended primarily for audiobooks and believe me opus codec is excellent choice there even in quite low bitrates. However if you want to change parameters of these three transcodings you can easily do so by providing yaml confing file to argument `--config`. Here is example of transcoding section in config file:

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

In each key first you have specification of codec-container combination, currently we support `opus-in-ogg`, `opus-in-webm`, `mp3`, `aac-in-adts` (but other containers or codecs can be relatively easily added, provided they are supported by ffmpeg and container creation does not require seekable output - like MP4 container).

I have good experiences with `opus-in-ogg`, which is also default. `opus-in-webm` works well in browsers (and is supported in browsers MSE API), but as it does not contain audio duration after transcoding, audio cannot be sought during playback in Android client, which is significant drawback. `mp3` is classical MPEG-2 Audio Layer III audio stream. `aac-in-adts` is AAC encoded audio in ADTS stream, it also may have problems with seeking in Android client.

For opus transcodings there are 3 other parameters, where `bitrate` is desired bitrate in kbps, `compression_level` is determining audio quality and speed of transcoding with values 1-10 ( 1 - worst quality, but fastest, 10 - best quality, but slowest ) and `cutoff` is determining audio freq. bandwidth (NarrowBand => 4kHz, MediumBand => 6kHz, WideBand => 8kHz, SuperWideBand => 12kHz, FullBand => 20kHz).

For mp3 transcoding there are also 3 parameters: `bitrate` (in kbps), `compression_level` with values 0-9 (0 - best quality, slowest, 9-worst quality, fastest; so meaning is inverse then for opus codec) and `abr` (optional), which can be `true` or `false` (ABR = average bit rate - enables ABR, which is similar to VBR, so it can improve quality on same bitrate, however can cause problems, when seeking in audion stream).

`aac-in-adts` has one mandatory parameter `bitrate` (in kbps) and two optional parameters `sr` - which is sample rate of transcoded stream (8kHz, 12kHz, 16kHz, 24kHz, 32kHz, 48kHz, unlimited) and `ltp` (Long Term Prediction), which is `true` or `false` and can improve audio quality, especially for lower bitrates, but with significant performance costs ( about 10x slower).

All encodings have optional parameter `mono`, if set to `true` audio will be down-mixed to mono.

You can override one two or all three defaults, depending on what sections you have in this config file. You can also provide complete alternative transcoding configuration for particular clients ([see below](#alternative-transcodings-and-transcoding-configuration-for-apple-users))

Overall `opus-in-ogg` provides best results from both quality and functionality perspective, so I'd highly recommend to stick to it, unless you have some problem with it, which might be case on Apple platforms ([see below](#alternative-transcodings-and-transcoding-configuration-for-apple-users)).


### Alternative transcodings and transcoding configuration for Apple users

Default transcoding for audioserve is opus codec in ogg container, which is not supported on Apple platforms. Recently audioserve also supports alternative transcoding configurations based on matching User-Agent string in request header. You can create any number of alternative transcoding configurations, each identified by a regular expression. First matching configuration is then used.

So if you create this configuration file:

```
---
transcoding:
  alt_configs:
    "iPhone|IPad|Mac OS":
      low:
        aac-in-adts:
          bitrate: 32
          sr: "24kHz"
          mono: true
      medium:
        aac-in-adts:
          bitrate: 48
          mono: false
      high:
        aac-in-adts:
          bitrate: 64
          mono: false

```

and use it with audioserve through argument `--config` or short version `-g`. 
It will then use aac transcoding for browsers on Apple platforms.

## Command line

Audioserve can take parameters from command line, environment variables and config file. For command line arguments check them with `audioserve -h`. Generally you need to provide shared secrect (or option `--no-authentication` for public access) and media collection directory (as noted above you can have severals collections). You can also provide options specific for particular collection directory (add : and options directly after the collection path). For details use `help-dir-options` argument.

`audioserve` is server executable and it also needs web client files , which are `index.html` and `bundle.js`, which are defaultly in `./client/dist`, but their location can by specified by argument `-c`.

For majority of command line arguments there are also appropriate environment variables, which start with prefix `AUDIOSERVE_` and then command line argument name (without leading dashes) transcribed from kebab-case to SCREAMING_SNAKE_CASE, so for instance argument `--shared-secret` has corresponding env. variable `AUDIOSERVE_SHARED_SECRET`.

All audioserve parameters can be also provided in configuration file via `--config` argument. Configuration file is in YAML format and somehow resembles command line arguments, but not exactly (main difference is dashes are replaced by underscores). Easiest way how to create config file is to use argument `--print-config`, which prints current configuration, including all used arguments to standard output.

## Web client

Finally I think **new web client**  is ready for prime time, so I'll become default - it resides in it's [own project](https://github.com/izderadicka/audioserve-web) and it's integrated into Docker image build, so it's part of the image (TBD for stable image and static release). New web client uses latest and greatest web technologies and it's intended to replace Android client (can be installed as PWA app). However if you do not like new client for any reason (please let me know what's wrong with new client), you can still use old client (residing in this repo, it's just HTML5 and javascript, so less demanding, but also code is bit clumsy), which will be around for some time. You can easily enable in Docker build with `OLD_CLIENT` build argument, or just build separately with npm and direct do resulting `dist` directory with `--client-dir` argument.


I'm testing web clients on recent Firefox and Chrome/Chromium (on Linux and Android platforms, occasionally on Win and Edge, assuming that Edge is now basically Chrome, so it should work). For Apple platforms, new client should work for Safari after some additional configuration - check [this chapter](#alternative-transcodings-and-transcoding-configuration-for-apple-users).


Also there is third party client, still very much in progress (and not much progressing lately), but quite interesting [third party client](https://github.com/KodeStar/audiosilo).


## Android client

Android client code is [available on github](https://github.com/izderadicka/audioserve-android).

## API

audioserve server provides very simple API, [defined in OAS 3](https://validator.swagger.io/?url=https://raw.githubusercontent.com/izderadicka/audioserve/master/docs/audioserve-api-v1.yaml) (see also [api.md](./docs/api.md) for details), so it's easy to write your own clients.

## Installation

### Docker Image

Easiest way how to test audioserve (but do not use `--no-authentication` in production) is to run it as docker container with prebuild [Docker image](https://cloud.docker.com/u/izderadicka/repository/docker/izderadicka/audioserve) (from Docker Hub). To quickly test audioserve run:

    docker run -d --name audioserve -p 3000:3000 -v /path/to/your/audiobooks:/audiobooks  izderadicka/audioserve --no-authentication /audiobooks

Then open <http://localhost:3000> - and browse your collection. This is indeed the very minimal configuration of audioserve and **should not be used in production**. For real deployment you'd like provide provide more command line arguments (or environment variables or your custom config file) and it's **essential** to map persistent volume or bind writable host directory to audioserve data-dir (defaulted to /home/audioserve/.audioserve or can be set via `--data-dir` argument) - see more complex example below.

There is also `izderadicka/audioserve:unstable` image, which is automatically built overnight from current master branch (so it contains latest features, but may have some issues). And of course you can build your own image very easily with provided `Dockerfile`, just run:

    docker build --tag audioserve .

When building docker image you can use `--build-arg CARGO_ARGS=` to modify cargo build command and to add/or remove features (see [below](#compiling-without-default-features-or-with-non-default-features) for details). For instance this command will build audioserve with transcoding cache `docker build --tag audioserve --build-arg CARGO_ARGS="--features transcoding-cache" .` If you want to build a debug version supply this build argument `--build-arg CARGO_RELEASE=""`.

There is also one additional env. variable `PORT` - TCP port on which audioserve serves http(s) requests (defaults to: 3000) - this is useful for services like Heroku, where container must accept PORT variable from the service.

A more realistic docker example (audioserve executable is an entry point to this container, so you can use all command line arguments of the program, to get help for the program run this command `docker run -it --rm izderadicka/audioserve --help`):

    docker run -d --name audioserve -p 3000:3000 \
        -v /path/to/your/audiobooks1:/collection1 \
        -v /path/to/your/audiobooks2:/collection2 \
        -v /path/for/audioserve-data:/home/audioserve/.audioserve \
        -e AUDIOSERVE_SHARED_SECRET=mypass \
        izderadicka/audioserve \
        --tags \
        --behind-proxy \
        --transcoding-max-parallel-processes 24 \
        --positions-backup-file /home/audioserve/.audioserve/positions-backup.json \
        --positions-backup-schedule "0 3 * * *" \
        /collection1 /collection2

In the above example, we are adding two different collections of audiobooks (collection1 and collection2).
Both are made available to the container via `-v` option and then passed to audioserve on command line.
Also we have mapped with `-v` some folder to `/home/audioserve/.audioserve`, where runtime data of audioserve are stored (server secret, caches ...). For production it's **essential** to map this volume either to host directory (which must have read and write permissions for audioserve user, id 1000 by default) or to named volume.

We set the shared secret via `AUDIOSERVE_SHARED_SECRET` env.variable and also set couple of other arguments:

- `--tags` to scan and cache common metadata tags.
- `behind-proxy` just improves logging - logs real client IP address if audioserve is behind reverse proxy
- `--transcoding-max-parallel-processes` increases a bit number of parallel transcoding allowed
- `--positions-backup-file` and `--positions-backup-schedule` backs up playback positions to open, transferrable JSON file

#### Running audioserve in Docker as different user

By default audioserve is running as user uid 1000, which is fine in many use cases (uid 1000 is default primary user on Debian, Ubuntu, Alpine linux, but for instance not on CentOS or RHEL).
But sometimes you'd like to run audioserve as different user (say uid 1234), for this you must:

- run docker with `--user 1234`
- assure that uid 1234 has read access to your collection folder (others have r access to files and rx to directories)
- create directory with read and write access for uid 1234 (or optionally use named volume)
- map that directory as volume to docker `-v /my/audioserve/data-dir:/audioserve-data`
- and add this argument to audioserve `--data-dir /audioserve-data`

#### Docker Compose Example

Here is a simple docker-compose example that enables tags caching

```yaml
---
version: "3"

services:
  audioserve:
    image: izderadicka/audioserve
    restart: unless-stopped
    command: --tags /audiobooks
    environment:
      - PUID=1000
      - PGID=1000
      - "AUDIOSERVE_SHARED_SECRET=VGM4oDS6wGKge9"
    volumes:
      - /etc/localtime:/etc/localtime:ro
      - "./config:/home/audioserve/.audioserve"
      - "/path/to/Audio/Books:/audiobooks"
```

### Static build (Linux)

Static build of audioserve is available (for recent releases) at [github releases page](https://github.com/izderadicka/audioserve/releases). You can can just download and extract it locally and run on any modern x86_64 linux.
You can also create your own static build with script `build_static.sh` (Docker is required for this)

### Local build (Linux)

Now audioserve depends on ffmpeg's libavformat 4.3/4.4 (and its dependent libavutil and libavcodec libs), which is a complex beast. If you are building locally you need this dependence (plus couple of others). If you have available right version on your system you can dynamically link against it (remember it has to be correct version, if you have wrong wersion you'll probably see Segmentation Faults when running the program). Other option is to use feature `partially-static`, which will download right version of ffmpeg, compile it and statically link it into audioserve (but then binary will be indeed bigger).

Install required dependencies (some dependencies are optional, depending on features chosen in build):

    # Ubuntu 20.04 - for other distros look for equivalent packages
    sudo apt-get install -y  git openssl libssl-dev pkg-config \
        ffmpeg yasm build-essential curl wget libbz2-dev zlib1g-dev libavformat-dev \
        clang coreutils exuberant-ctags gawk  libclang-dev llvm-dev strace libicu-dev


Clone repo with:

    git clone https://github.com/izderadicka/audioserve

To install locally you need recent [Rust](https://www.rust-lang.org/en-US/install.html) and [NodeJS](https://nodejs.org/en/download/package-manager/) installed.

Compile Rust code (it has optional system dependencies to openssl,zlib, bz2lib, and libavformat, as might not have exactly correct version of libavformat around, it's better to build required version statically into binary with `partially-static` feature, because otherwise you might see problems like segfaults):

    cargo build --release --features partially-static

Optionally you can compile with/without other features ([see below](#compiling-without-default-features-or-with-non-default-features) for details).

Build new client:
```
git clone https://github.com/izderadicka/audioserve-web.git /audioserve_client &&\
    cd /audioserve_client &&\
    npm install &&\
    npm run build &&\
    npm run build-sw &&\
    mv public dist  # The directory that audioserve will recognize as --client-dir
```

Optionally you can build old client (in its directory `cd client`):

    npm install
    npm run build

### Other platforms

Linux is the only officially supported platform. It can theoretically work on other platforms (win, MacOS), but it might require some changes in build process and probably also small changes in code. A contributor tried [build for windows](docs/windows-build.md) with partial success, so you can checked that.

Any help in this area is welcomed.

### Compiling without default features or with non-default features

TLS support (feature `tls`), symbolic links (feature `symlinks`), shared playback positions (feature `shared-positions`), enhanced logging, when behind proxy (feature `behind-proxy`) and folder download (feature `folder-download`) are default features, but you can compile without them - just add `--no-default-features` option to `cargo build` command. And then eventually choose only features you need.
To add non-default features (like `transcoding-cache`) compile with this option `--features transcoding-cache` in `cargo build` command.

**Available features:**

| Feature                       | Description                                                                                                                        | Default | Program options                                                                                                  |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | :-----: | ---------------------------------------------------------------------------------------------------------------- |
| tls                           | Enables TLS support (e.g https) (requires openssl and libssl-dev)                                                                  |   Yes   | --ssl-key --ssl-key-password to define server key                                                                |
| symlinks                      | Enables to use symbolic links in media folders                                                                                     |   Yes   | Use --allow-symlinks to follow symbolic links                                                                    |
| folder-download               | Enables API endpoint to download content of a folder in tar archive                                                                |   Yes   | Can be disabled with argument --disable-folder-download                                                          |
| shared-positions              | Clients can share recent playback positions via simple websocket API                                                               |   Yes   |
| behind-proxy                  | Enable logging of client address from proxy headers                                                                                |   yes   | Enables argument --behind-proxy which should be use to log client address from headers provided by reverse proxy |
| transcoding-cache             | Cache to save transcoded files for fast next use                                                                                   |   No    | Can be disabled by --t-cache-disable and modified by --t-cache-dir --t-cache-size --t-cache-max-files            |
| static                        | Enables fully static build of audioserve. Check above notes for static build                                                       |   No    |
| partially-static              | Statically links libavformat (and related).Enables to run audioserve on systems, which do not have required version of libavformat |   No    |
| folder-download-default-tar   | Default folder download format is tar (instead of zip)                                                                             |   No    |
| collation or collation-static | Supports locale collation (for static build second option must be used!)                                                           |   No    | Env. variables AUDIOSERVE_COLLATE, LC_ALL, LC_COLLATE, LANG determine locale used                                |
| tags-encoding                 | Enables alternate charactacters encoding for audio metadata tags                                                                   |   No    | Enables argument --tags-encoding                                                                                 |

## License

[MIT](https://opensource.org/licenses/MIT)
