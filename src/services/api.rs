use std::ffi::OsStr;
use std::{path::PathBuf, sync::Arc};

use collection::FoldersOrdering;
use futures::prelude::*;
use headers::{ContentLength, ContentType};
use hyper::{Body, Response as HyperResponse};
use tokio::task::spawn_blocking as blocking;

use crate::Error;
use crate::{config::get_config, util::ResponseBuilderExt};

use super::compress::compressed_response;
use super::search::{Search, SearchTrait};
use super::types::Transcodings;
use super::{response, response::ResponseFuture, types::CollectionsInfo};

type Response = HyperResponse<Body>;

fn json_response<T: serde::Serialize>(data: &T, compress: bool) -> Response {
    let json = serde_json::to_string(data).expect("Serialization error");

    let builder = HyperResponse::builder().typed_header(ContentType::json());
    if compress && json.len() > 512 {
        compressed_response(builder, json.into_bytes())
    } else {
        builder
            .typed_header(ContentLength(json.len() as u64))
            .body(json.into())
            .unwrap()
    }
}

pub fn get_folder(
    collection: usize,
    folder_path: PathBuf,
    collections: Arc<collection::Collections>,
    ordering: FoldersOrdering,
    group: Option<String>,
    compress: bool,
) -> ResponseFuture {
    Box::pin(
        blocking(move || collections.list_dir(collection, &folder_path, ordering, group))
            .map_ok(move |res| match res {
                Ok(folder) => json_response(&folder, compress),
                Err(_) => response::not_found(),
            })
            .map_err(Error::new),
    )
}

const UNKNOWN_NAME: &str = "unknown";

pub fn collections_list(compress: bool) -> ResponseFuture {
    let collections = CollectionsInfo {
        version: env!("CARGO_PKG_VERSION"),
        folder_download: !get_config().disable_folder_download,
        shared_positions: cfg!(feature = "shared-positions"),
        count: get_config().base_dirs.len() as u32,
        names: get_config()
            .base_dirs
            .iter()
            .map(|p| {
                p.file_name()
                    .and_then(OsStr::to_str)
                    .unwrap_or(UNKNOWN_NAME)
            })
            .collect(),
    };
    Box::pin(future::ok(json_response(&collections, compress)))
}

#[cfg(feature = "shared-positions")]
pub fn insert_position(
    collections: Arc<collection::Collections>,
    group: String,
    bytes: bytes::Bytes,
) -> ResponseFuture {
    match serde_json::from_slice::<collection::Position>(&bytes) {
        Ok(pos) => {
            let path = if !pos.folder.is_empty() {
                pos.folder + "/" + &pos.file
            } else {
                pos.file
            };
            Box::pin(
                collections
                    .insert_position_if_newer_async(
                        pos.collection,
                        group,
                        path,
                        pos.position,
                        pos.folder_finished,
                        pos.timestamp,
                    )
                    .then(|res| match res {
                        Ok(_) => response::fut(response::created),
                        Err(e) => match e {
                            collection::error::Error::IgnoredPosition => {
                                response::fut(response::ignored)
                            }
                            _ => Box::pin(future::err(Error::new(e))),
                        },
                    }),
            )
        }
        Err(e) => {
            error!("Error in position JSON: {}", e);
            response::fut(response::bad_request)
        }
    }
}

#[cfg(feature = "shared-positions")]
pub fn last_position(
    collections: Arc<collection::Collections>,
    group: String,
    compress: bool,
) -> ResponseFuture {
    Box::pin(
        collections
            .get_last_position_async(group)
            .map(move |pos| Ok(json_response(&pos, compress))),
    )
}

#[cfg(feature = "shared-positions")]
pub fn folder_position(
    collections: Arc<collection::Collections>,
    group: String,
    collection: usize,
    path: String,
    recursive: bool,
    filter: Option<collection::PositionFilter>,
    compress: bool,
) -> ResponseFuture {
    if recursive {
        Box::pin(
            collections
                .get_positions_recursive_async(collection, group, path, filter)
                .map(move |pos| Ok(json_response(&pos, compress))),
        )
    } else {
        Box::pin(
            collections
                .get_position_async(collection, group, path)
                .map(move |pos| Ok(json_response(&pos, compress))),
        )
    }
}

#[cfg(feature = "shared-positions")]
pub fn all_positions(
    collections: Arc<collection::Collections>,
    group: String,
    filter: Option<collection::PositionFilter>,
    compress: bool,
) -> ResponseFuture {
    Box::pin(
        collections
            .get_all_positions_for_group_async(group, filter)
            .map(move |pos| Ok(json_response(&pos, compress))),
    )
}

pub fn transcodings_list(user_agent: Option<&str>, compress: bool) -> ResponseFuture {
    let transcodings = user_agent
        .map(Transcodings::for_user_agent)
        .unwrap_or_default();
    Box::pin(future::ok(json_response(&transcodings, compress)))
}

pub fn search(
    collection: usize,
    searcher: Search<String>,
    query: String,
    ordering: FoldersOrdering,
    group: Option<String>,
    compress: bool,
) -> ResponseFuture {
    Box::pin(
        blocking(move || {
            let res = searcher.search(collection, query, ordering, group);
            json_response(&res, compress)
        })
        .map_err(Error::new),
    )
}

pub fn recent(
    collection: usize,
    searcher: Search<String>,
    group: Option<String>,
    compress: bool,
) -> ResponseFuture {
    Box::pin(
        blocking(move || {
            let res = searcher.recent(collection, group);
            json_response(&res, compress)
        })
        .map_err(Error::new),
    )
}
