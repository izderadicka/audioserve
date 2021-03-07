audioserve API
==============

audioserve API is simple HTTP API with mostly JSON loads. For updating/quering recent playback positions WebSocket connection is used with simple text based messages.

Authetication
-------------

A token is used for authentication, the token can be used as cookie with key `audioserve_token` 
or as HTTP header `Authorization: Bearer token_value`.  Token is signed by server secret key and contains
maximum validity time (token validity period can be set on the audioserve server) . If no or invalid token is provided
API endpoints return 401 Unauthorised HTTP response code.

Token is received from server when client proves knowledge of shared secret. For this api endpoint `authenticate` is available.

**authenticate**

Sample URL: https://your_server_name:3000/authenticate

POST request, Content-Type: application/x-www-form-urlencoded, parameter `secret`, which contains salted shared secret

Response code 200, Content-Type: text/plain,  whole response is the token  
Response code 401 Unauthorised - invalid shared secret  

Salted shared secret is calculated as:

* shared secret is encoded as UTF-8 bytes
* client generates 32 random bytes
* secret is concatenated with random bytes (secret+random_bytes)
* these bytes are hashed with SHA-256
* random bytes are encoded with base64 encoding
* hash is encoded with base64 encoding
* resulting secret is string concation of "encoded random" + "|" + "encoded hash"

API endpoints
----------------------

All are GET requests with valid token, 401 Unauthorised is returned, if token is missing or invalid

**collections**

Sample URL: https://your_server_name:3000/collections

Returns list of available collections (collection is a directory provided as parameter to audioserve server). 

```json
    {
    "folder_download": true,
    "count":2,
    "names":[
        "test_audiobooks_eng",
        "test_audiobooks"]
        }
```

`folder_download` - is folder download is enabled on the server
`count` - number of collections
`names` - array of collection names

**transcodings**

Sample URL: https://your_server_name:3000/transcodings

Gets current transcoding settings.

```json
    {
    "max_transcodings":8,
    "low":{
        "bitrate":32,
        "name":"opus-in-ogg"
        },
    "medium":{
        "bitrate":48,
        "name":"opus-in-ogg"
        },
    "high":{
        "bitrate":64,
        "name":"opus-in-ogg"
        }
    }
```

There are 3 possible level of transcoding `low`, `medium`, `high`, each with name of transcoding and expected for resulting data stream - for details about transcoding general [README.md](../README.md). `max_transcodings` is maximum number of trancoding processes, that can run on server in parallel. If this maximum is reached server returns 503 Service Unavailable -  it's client responsibility to retry later.

**folder**

Sample URL: https://your_server_name:3000/folder/  
Sample URL: https://your_server_name:3000/folder/author_name/audio_book  
Sample URL: https://your_server_name:3000/1/folder/author_name/audio_book?ord=m  

Lists available subfolders or audio files in the folder. Path starts either with `/collection number` + `/folder/` 
 (list of collections can be retrieved by API endpoint `collections`) or directly with `/folder/`, which uses then 
 collection 0 as default.
 URL has optional query parameter `ord`, meaning ordering of subfolders, two values are now supported:

* `a` alphabetical
* `m` recent first (using folder mtime)

Returns JSON:

 ```json
 {
    "files":
        [{
            "name":"5 pomerancovych jaderek.mp3",
            "path":"Doyle Arthur Conan/5 pomerancovych jaderek.mp3",
            "meta":{
                "duration":2518,
                "bitrate":80
                },
            "mime":"audio/mpeg",
            "section": null
        },
        {
            "name":"Barvir na penzi.opus",
            "path":"Doyle Arthur Conan/Barvir na penzi.opus",
            "meta":{
                "duration":1612,
                "bitrate":31},
            "mime":"audio/ogg",
            "section": null
        }],
    "subfolders":
        [{
            "name":"Berylova korunka",
            "path":"Doyle Arthur Conan/Berylova korunka",
            "is_file": false
        },
        {
            "name":"Domaci pacient",
            "path":"Doyle Arthur Conan/Domaci pacient",
            "is_file": false
        }],
    "cover":
        {
            "path":"Doyle Arthur Conan/Arthur_Conany_Doyle_by_Walter_Benington,_1914.png",
            "mime":"image/png"
        },
    "description":
        {
            "path":"Doyle Arthur Conan/author.html",
            "mime":"text/html"
        }
}
```

Data contains `files` and/or `subfolders` (each can be null or empty array). Subfolders can be listed using this API endpoint
using path `/folder/` + `path` (for collection 0) or `/x/folder/` + `path` (for collection x). Root folder of collection is retrieved with path `/folder/` (collection 0) or `/x/folder` (for colections x, x>=0).

`files` contains playable files -  `path` should be used with `audio` endpoint - see below - in similar way as in listing subfolders.  `meta` contains some metadata about audio file - `duration` in seconds and `bitrate` in kbps. `mime` is mime type
of the audio file and `section` is only used with chapters extracted from single file audiobook (then it contains `start` and `duration` of the chapters in ms).

`subfolders` entries contain also field `is_file`, which is true for single file chaptered audibooks (.m4b format for instance) that are presented as folders. Listing of such file's chapters is done via this endpoint.  The only difference against regular directory is that artificial file entries are created for chapters - name is chapter name and path is in form `path/to/audiobook.m4b/Chapter_name$$1000-2000$$.m4b`, where numbers between `$$` are start and end of the chapter in milliseconds. There is also alternative form of path when containing directory is collapsed/skipped `path/to/audiobook.m4b$$Chapter_name$$1000-2000$$.m4b` using `$$` separator. Also each such file has data in `section` key with start of chapter and its duration in milliseconds. Here is example of such entry:

```json
{
    "name":"000 - Chapter_1",
    "path":"Stoker Bram/Dracula/Dracula.m4b>>000 - Chapter_1$$0-1020288$$.m4b",
    "meta":{"duration":1020,"bitrate":54},
    "mime":"audio/m4b",
    "section":{"start":0,"duration":1020288}}
```

Folder can contain additional information `cover`, which is cover image (first .jpg or .png file encountered in the folder) and text information `description` (first .txt, .html, .md file encoutered in the folder). Both can be null, if there is no appropriate file and if not null, file can be retrieved in appropriate API end point by using the `path`.


**download**

Sample URL: https://your_server_name:3000/download/author_name/audio_book  
Sample URL: https://your_server_name:3000/1/download/author_name/audio_book?fmt=tar

Downloads all files (audio files, cover, description) from this folder as an archive. The path to folder is same as in folder list endpoint `folder` - so it can start with collection number. Default format of the archive is zip, tar archive is also supported - format can be chosen by `fmt` query parameter (values `tar` or `zip`). Also if you want to change default format by compiling audioserve with `folder-download-default-tar` feature.

This endpoint can be disabled, if audioserve is compiled without default feature `folder-download` or with command line argument `--disable-folder-download` .

**search**

Sample URL: https://your_server_name:3000/search?q=holmes  
Sample URL: https://your_server_name:3000/1/search?q=adams&ord=m

Searches collection - only one collection is searched - so it can be prefixed with with collection number to search right collection (`/x/search`).  Search term is in query string paramater `q`. It returns list of folders (and files,
but search is not implemented for file names now), which can be used is same way as in folder listing.
URL can contain optional `ord` query parameter, meaning ordering of results, same as in `folder` endpoint.

```json
{
    "files":[],
    "subfolders":
    [{
        "name":"The Adventures of Sherlock Holmes",
        "path":"Doyle, Arthur Conan/The Adventures of Sherlock Holmes"
    },
    {
        "name":"The Return of Sherlock Holmes",
        "path":"Doyle, Arthur Conan/The Return of Sherlock Holmes"
    }]
}
```

Currently search is implemented only for folder names. Search term is split to words and each word is searched in full path (relative collection root - the path you see in folder listing).
First path that includes all words in added to results (and it's subfolders are not searched further).

**recent**
Sample URL: https://your_server_name:3000/recent  
Sample URL: https://your_server_name:3000/1/recent

Lists top 64 most recent folders in the collection (based on folder modification time). Returns same json object as previous API endpoint `search`, but here subfolders are sorted by folder modification time descendently - e.g most recent is first.

**audio**

Sample URL: https://your_server_name:3000/audio/Doyle Arthur Conan/5 pomerancovych jaderek.mp3  
Sample URL: https://your_server_name:3000/audio/Doyle Arthur Conan/5 pomerancovych jaderek.mp3?trans=m  
Sample URL: https://your_server_name:3000/audio/Doyle Arthur Conan/5 pomerancovych jaderek.mp3?trans=m&seek=537.42  
Sample URL: https://your_server_name:3000/2/audio/author_name/series_name/audiobook_name/chapter1.opus

This endpoint allows to access audio file, either as it is stored audio file (in this case [http bytes range](https://developer.mozilla.org/en-US/docs/Web/HTTP/Range_requests) is supported - but only one range per response)  
or transcoded content of the file (in this case response content is [chunck encoded](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Transfer-Encoding) and its length is not known in advance). 
It's responsibility of client to choose direct or transcoded content as needed. 

Transcoding is triggered by query string paramater `trans`, which can have one of three possible values `l` (Low profile), 
`m` (Medium profile) and `h` (High profile) - for meaning of transcoding profiles see above API endpoints `transcodings`.
Typical usecase is that client loads transcoding parameters from `transcodings` endpoint and then for each audio file decides
if transcoding is required or not based on `mime` and `bitrate` values available in folder listing.

Transcoded files can be also seek for -  query string parameter `seek` can contain start of stream in seconds (related to 
normal begining of file).  Plain, not transcoded files cannot be seeked in this way (they support byte ranges, which are 
usually enough for a player to seek efficiently). So `seek` can be used only with `trans`.

As already mentioned above, number of transcoding processing is limited, as it is lengthy and resources demanding (mainly CPU) 
process. If maximum number of transcodings is already used, this endpoint will return HTTP response 503 Service Unavailable. It's client responsibility to handle such cases. 

**cover**

Sample URL: https://your_server_name:3000/cover/Doyle Arthur Conan/Arthur_Conany_Doyle_by_Walter_Benington,_1914.png  
Sample URL: https://your_server_name:3000/2/cover/author_name/series_name/audiobook_name/cover.jpg  

If cover exists in the folder, this enpoint enables to load the image.

**desc**

Sample URL: https://your_server_name:3000/desc/Doyle Arthur Conan/author.html  
Sample URL: https://your_server_name:3000/2/desc/author_name/series_name/audiobook_name/info.txt  

If text information in the folder, this endpoint can load the text.

Recent playback position
------------------------

Clients can share and query recent playback positions. In order to determine, which clients share positions, there is a concept of clients group. Group is just arbitrary name and clients using same group name will share recent playback positions between them.

Playback positions are reported and queried via websocket connection to audioserve server on path `/position` - so sample websocket url can look like `wss://you_server_name:3000/position`. Clients send two types of text messages (and there is no specific websocket subprotocol):

- **current playback position** - when client is playing audiofile, it can send current playback position in regular intervals (provided clients use 10 secs interval), group name and audio file path
- **query for last positions** - when user wants to continue playback on client, it can check what where latest positions within the group.

### Current playback position message ###
Client sends simple text message:

    playback_time_secs|group_name/collection_number/audio_file_path

Example of such message is (group here is name of group, can be anything):

    480.383|group/0/Adams Douglas/Douglas Adams - Stoparuv pruvodce galaxii (2008)/01.kapitola.mp3

Client can send also longer form of this message:

    playback_time_secs|group_name/collection_number/audio_file_path|timestamp

This longer form is useful, if connection is interrupt and client wants to report on position which was reached in past (however it's taken into consideration if there is no newer position in the group). Timestamp is unix time in seconds. Example of such message:

    480.383|group/0/Adams Douglas/Douglas Adams - Stoparuv pruvodce galaxii (2008)/01.kapitola.mp3|1614963001

Such message can be send also in short form, without  `group_name/collection_number/audio_file_path`, if we are continuing to report position in same audio file. So next position update could be just:

    486.859|

Notice the trailing pipe - it's essential (to distinguish from other message type). Also this short type can be used only if long form was first sent on same websocket connection (e.g. currently reported file is in context of websocket connection from client).

### Querying last positions messages ###

Client can also query last playback position in the group, websocket message should look like:

    group_name/collection_number/audio_folder_path

So basically same path like string, but without last component - the audio file. audio_folder_path is current folder opened in the client, used to get also latest position in this folder (if there is any). Example of such query:

    group/0/Adams Douglas/Douglas Adams - Stoparuv pruvodce galaxii (2008)

If client is not showing any specific folder, it can send generic query, just to get very last position in any folder, in that case it can send just empty string or `?` string.

Response to this query is again websocket text message, this time containing JSON object:

    {
        folder: {position_object or null},
        last: {position_object or null}
    }

`folder` key is last position in queried folder and `last` key is latest position in all collections (in given clients group, each group is separate). If both positions are same, only one is send back, other is null. If query is generic (not specific to folder) or there is not any known last position in the folder, only `last` is returned, `folder` is null. Both can be null only if there are no positions reported yet in this group. Position object looks like this:

    {
        "file": "audio_file_name",
        "folder": "audio_folder_path_without_collection_and_group",
        "timestamp": miliseconds_from_epoch,
        "position": playback_position_in_secs,
        "collection": collection_number
    }

Example of position query response:

    {
        "folder":{
            "file":"01.kapitola.mp3",
            "folder":"Adams Douglas/Douglas Adams - Stoparuv pruvodce galaxii (2008)","timestamp":1558016643841,
            "position":486.859,
            "collection":0},
        "last":null
    }
