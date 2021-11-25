audioserve API
==============

audioserve API is simple HTTP REST  API with (mostly) JSON loads. For efficient playback positions updates you can 
use text based [web socket API](#websocket-playback-position-api), but there is also convenient [REST API for last positions update/query](#positions-api).

REST API
--------
Rest API for audioserve is defined in [OpenAPI 3 Specification](audioserve-api-v1.yaml).

You can also see [specification in Swagger UI](https://validator.swagger.io/?url=https://raw.githubusercontent.com/izderadicka/audioserve/master/docs/audioserve-api-v1.yaml). You can use it also test API on demo instance of audioserve:
- Choose "https://audioserve.zderadicka.eu" server
- Expand Authentication POST endpoint, and click on "Try it out" button.
- use sample secret provided and click big blue "Execute" button
- you should get success 200 response with token in response body,  copy it
- and client green "Authorize" button on top of the page,  paste token there and client "Authorize" and then "Close" buttons
- now you should be able to test other API endpoints, which require authorization


Authentication API
------------------

A token is used for authentication, the token can be used as cookie with key `audioserve_token` name
or as HTTP header `Authorization: Bearer token_value`.  Token is signed by server secret key and contains
maximum validity time (token validity period can be set on the audioserve server). 
If no or invalid token is provided API endpoints return `401 Unauthorised` HTTP response code.

Token is received from server when client proves knowledge of shared secret. For this api endpoint `authenticate` is available. For details see also [OAS3 endpoint /authenticate](audioserve-api-v1.yaml).


Collections API
----------------

Here are some additional information, that cannot be included in [OAS3 Specification](audioserve-api-v1.yaml).

Some endpoints are specific for given collection - so their path starts with parameter `col_id`.  
Actually for historical reasons this parameter is optional and collection 0 is then default, however it's now recommended to be explicit.

If you tested API in swagger you probably noticed that `path` parameter is fully URL encoded - eg. path separator is encoded as %2F. It works, but actually it is not required, `path` can use it's separators directly, thus be a natural extension of URL path (but of course path segments must be URL friendly, so URL encoding is needed for these). Same holds for `path` parameter in Positions API.


### Note on chaptered audiofiles (.m4b and similar)

Chaptered audibooks (.m4b format for instance)  are presented as folders. Listing of such file's chapters is done via `/{col_id}/folder/{path}` endpoint.  
The only difference against regular directory is that artificial file entries are created for chapters - name is chapter name and path is in form `path/to/audiobook.m4b/Chapter_name$$1000-2000$$.m4b`, where numbers between `$$` are start and end of the chapter in milliseconds. 
There is also alternative form of path when containing directory is collapsed/skipped, which is used to truncate the path if there is only one such audiofile in the directory: `path/to/audiobook.m4b$$Chapter_name$$1000-2000$$.m4b` using `$$` separator (notice $$ is used instead of path separator). Also each such file has data in `section` key with start of chapter and its duration in milliseconds. Here is example of such entry:

```json
{
    "name":"000 - Chapter_1",
    "path":"Stoker Bram/Dracula/Dracula.m4b>>000 - Chapter_1$$0-1020288$$.m4b",
    "meta":{"duration":1020,"bitrate":54},
    "mime":"audio/m4b",
    "section":{"start":0,"duration":1020288}}
```


Positions API
-------------

Positions API is also described in [OAS3 yaml file](audioserve-api-v1.yaml),  it's REST API, which enables to store and query last playback position per folder and per group (concept of sharing position with a group was explained in [main README](../README.md) or below).

There is also alternative (older) API using websocket and combination of custom text messages and JSON.  Main advantage of old API is it's wire efficiency, only few bytes are transferred for each position update. This older API is used in current clients.

Websocket playback position API
-------------------------------

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

Such message can be send also in short form, without  `group_name/collection_number/audio_file_path`, if we are continuing to report position on same audio file. So next position update could be just:

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
        "folder": "audio_folder_path_without_group_but_with_collection",
        "timestamp": miliseconds_from_epoch,
        "position": playback_position_in_secs
    }

Example of position query response:

    {
        "folder":{
            "file":"01.kapitola.mp3",
            "folder":"0/Adams Douglas/Douglas Adams - Stoparuv pruvodce galaxii (2008)",
            "timestamp":1558016643841,
            "position":486.859
            },
        "last":null
    }
