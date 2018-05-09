audioserve API
==============

audioserve API is simple HTTP API with mostly JSON loads.

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
* secret is concatated with random bytes (secret+random_bytes)
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
    "count":2,
    "names":[
        "test_audiobooks_eng",
        "test_audiobooks"]
        }
```

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
        "compression_level":5,
        "cutoff":"SuperWideBand"
        },
    "medium":{
        "bitrate":48,
        "compression_level":8,
        "cutoff":"SuperWideBand"
        },
    "high":{
        "bitrate":64,
        "compression_level":10,
        "cutoff":"FullBand"
        }
    }
```

There are 3 possible level of transcoding `low`, `medium`, `high`, each with parameters for opus audio codec - for details about parameters see general [README.md](../README.md). `max_transcodings` is maximum number of trancoding processes, that can run on server in parallel. If this maximum is reached server returns 503 Service Unavailable -  it's client responsibility to retry later.

**folder**

Sample URL: https://your_server_name:3000/folder/  
Sample URL: https://your_server_name:3000/folder/author_name/audio_book  
Sample URL: https://your_server_name:3000/1/folder/author_name/audio_book  

Lists available subfolders or audio files in the folder. Path starts either with `/collection number` + `/folder/` 
 (list of collections can be retrieved by API endpoint `collections`) or directly with `/folder/`, which uses then 
 collection 0 as default.

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
            "mime":"audio/mpeg"
        },
        {
            "name":"Barvir na penzi.opus",
            "path":"Doyle Arthur Conan/Barvir na penzi.opus",
            "meta":{
                "duration":1612,
                "bitrate":31},
            "mime":"audio/ogg"
        }],
    "subfolders":
        [{
            "name":"Berylova korunka",
            "path":"Doyle Arthur Conan/Berylova korunka"
        },
        {
            "name":"Domaci pacient",
            "path":"Doyle Arthur Conan/Domaci pacient"
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
using path `/folder/` + `path` (for collection 0) or `/x/folder/` + `path` (for collection x). Root folder of collection is retrieved with path `/folder/` (collection 0) or `/x/folder` (for colections x, x>=0)

`files` contains playable files -  `path` should be used with `audio` endpoint - see below - in similar way as in listing 
subfolders.  `meta` contains some metadata about audio file - `duration` in seconds and `bitrate` in kbps. `mime` is mime type
of the audio file.

Folder can contain additional information `cover`, which is cover image (first .jpg or .png file encountered in the folder) and text information `description` (first .txt, .html, .md file encoutered in the folder). Both can be null, if there is no appropriate file and if not null, file can be retrieved in appropriate API end point by using the `path`.

**search**

Sample URL: https://your_server_name:3000/search?q=holmes  
Sample URL: https://your_server_name:3000/1/search?q=adams

Searches collection - only one collection is searched - so it can be prefixed with with collection number to search right collection (`/x/search`).  Search term is in query string paramater `q`. It returns list of folders (and files,
but search is not implemented for file names now), which can be used is same way as in folder listing.

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

Currently search is implemented only for folder names. Search term is split to words and each word is searched in full path 
(relative collection root - the path you see in folder listing). 
First path that includes all words in added to results (and it's subfolders are not searched further).

**audio**

Sample URL: https://your_server_name:3000/folder/Doyle Arthur Conan/5 pomerancovych jaderek.mp3  
Sample URL: https://your_server_name:3000/folder/Doyle Arthur Conan/5 pomerancovych jaderek.mp3?trans=m  
Sample URL: https://your_server_name:3000/folder/Doyle Arthur Conan/5 pomerancovych jaderek.mp3?trans=m&seek=537.42  
Sample URL: https://your_server_name:3000/2/folder/author_name/series_name/audiobook_name/chapter1.opus

This endpoint allows to access audio file, either as it is stored (in this case [http bytes range](https://developer.mozilla.org/en-US/docs/Web/HTTP/Range_requests) is supported - but only one range per response)  
or transcoded (in this case response content is [chunck encoded](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Transfer-Encoding) and its length is not known in advance). 
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










