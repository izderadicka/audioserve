#![allow(dead_code)]
/**
 ffmpeg suported tags:

album        -- name of the set this work belongs to
album_artist -- main creator of the set/album, if different from artist.
                e.g. "Various Artists" for compilation albums.
artist       -- main creator of the work
comment      -- any additional description of the file.
composer     -- who composed the work, if different from artist.
copyright    -- name of copyright holder.
creation_time-- date when the file was created, preferably in ISO 8601.
date         -- date when the work was created, preferably in ISO 8601.
disc         -- number of a subset, e.g. disc in a multi-disc collection.
encoder      -- name/settings of the software/hardware that produced the file.
encoded_by   -- person/group who created the file.
filename     -- original name of the file.
genre        -- <self-evident>.
language     -- main language in which the work is performed, preferably
                in ISO 639-2 format. Multiple languages can be specified by
                separating them with commas.
performer    -- artist who performed the work, if different from artist.
                E.g for "Also sprach Zarathustra", artist would be "Richard
                Strauss" and performer "London Philharmonic Orchestra".
publisher    -- name of the label/publisher.
service_name     -- name of the service in broadcasting (channel name).
service_provider -- name of the service provider in broadcasting.
title        -- name of the work.
track        -- number of this work in the set, can be in form current/total.
variant_bitrate -- the total bitrate of the bitrate variant that the current stream is part of

Following tags are not from ffmpeg documentation, but work for some formats mpeg4 aka .m4b
series       -- name of the audiobook series
series_sequence -- specifies the part of the series

 */

pub const ALBUM: &str = "album";
pub const ALBUM_ARTIST: &str = "album_artist";
pub const ARTIST: &str = "artist";
pub const COMMENT: &str = "comment";
pub const COMPOSER: &str = "composer";
pub const COPYRIGHT: &str = "copyright";
pub const CREATION_TIME: &str = "creation_time";
pub const DATE: &str = "date";
pub const DISC: &str = "disc";
pub const ENCODER: &str = "encoder";
pub const ENCODED_BY: &str = "encoded_by";
pub const FILENAME: &str = "filename";
pub const GENRE: &str = "genre";
pub const LANGUAGE: &str = "language";
pub const PERFORMER: &str = "performer";
pub const PUBLISHER: &str = "publisher";
pub const SERVICE_NAME: &str = "service_name";
pub const SERVICE_PROVIDER: &str = "service_provider";
pub const TITLE: &str = "title";
pub const TRACK: &str = "track";
pub const VARIANT_BITRATE: &str = "variant_bitrate";
pub const SERIES: &str = "series";
pub const SERIES_SEQUENCE: &str = "series_sequence";

pub const ALLOWED_TAGS: &[&str] = &[
    ALBUM,
    ALBUM_ARTIST,
    ARTIST,
    COMMENT,
    COMPOSER,
    COPYRIGHT,
    CREATION_TIME,
    DATE,
    DISC,
    ENCODER,
    ENCODED_BY,
    FILENAME,
    GENRE,
    LANGUAGE,
    PERFORMER,
    PUBLISHER,
    SERVICE_NAME,
    SERVICE_PROVIDER,
    TITLE,
    TRACK,
    VARIANT_BITRATE,
    SERIES,
    SERIES_SEQUENCE,
];

pub const BASIC_TAGS: &[&str] = &[ALBUM, ARTIST, COMPOSER, DATE, GENRE, PERFORMER, TITLE];
