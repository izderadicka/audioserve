use std::ffi::OsStr;
use std::{path::PathBuf, sync::Arc};

use collection::FoldersOrdering;
use futures::prelude::*;
use tokio::task::spawn_blocking as blocking;

use super::search::{Search, SearchTrait};
use super::types::CollectionsInfo;
use super::types::Transcodings;
use crate::config::get_config;
use crate::Error;
use myhy::response::{self, json_response, ResponseResult};

pub async fn get_folder(
    collection: usize,
    folder_path: PathBuf,
    collections: Arc<collection::Collections>,
    ordering: FoldersOrdering,
    group: Option<String>,
    compress: bool,
) -> ResponseResult {
    blocking(move || collections.list_dir(collection, &folder_path, ordering, group))
        .map_ok(move |res| match res {
            Ok(folder) => json_response(&folder, compress),
            Err(_) => response::not_found(),
        })
        .map_err(Error::new)
        .await
}

pub async fn get_feed(
    collection: usize,
    collections: Arc<collection::Collections>,
    folder_path: PathBuf,
    compress: bool,
) -> ResponseResult {
    todo!("get_feed")
}

const UNKNOWN_NAME: &str = "unknown";

pub fn collections_list(compress: bool) -> ResponseResult {
    let collections = CollectionsInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: env!("AUDIOSERVE_COMMIT"),
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
    Ok(json_response(&collections, compress))
}

#[cfg(feature = "shared-positions")]
pub async fn insert_position(
    collections: Arc<collection::Collections>,
    group: String,
    bytes: bytes::Bytes,
) -> ResponseResult {
    match serde_json::from_slice::<collection::Position>(&bytes) {
        Ok(pos) => match collections.insert_position_if_newer_async(group, pos).await {
            Ok(_) => Ok(response::created()),
            Err(e) => match e {
                collection::error::Error::IgnoredPosition => Ok(response::ignored()),
                _ => Err(Error::new(e)),
            },
        },
        Err(e) => {
            error!("Error in position JSON: {}", e);
            Ok(response::bad_request())
        }
    }
}

#[cfg(feature = "shared-positions")]
pub async fn last_position(
    collections: Arc<collection::Collections>,
    group: String,
    compress: bool,
) -> ResponseResult {
    let pos = collections.get_last_position_async(group).await;
    Ok(json_response(&pos, compress))
}

#[cfg(feature = "shared-positions")]
pub async fn folder_position(
    collections: Arc<collection::Collections>,
    group: String,
    collection: usize,
    path: String,
    recursive: bool,
    filter: Option<collection::PositionFilter>,
    compress: bool,
) -> ResponseResult {
    if recursive {
        let pos = collections
            .get_positions_recursive_async(collection, group, path, filter)
            .await;
        Ok(json_response(&pos, compress))
    } else {
        let pos = collections
            .get_position_async(collection, group, path)
            .await;
        Ok(json_response(&pos, compress))
    }
}

#[cfg(feature = "shared-positions")]
pub async fn all_positions(
    collections: Arc<collection::Collections>,
    group: String,
    filter: Option<collection::PositionFilter>,
    compress: bool,
) -> ResponseResult {
    let pos = collections
        .get_all_positions_for_group_async(group, filter)
        .await;
    Ok(json_response(&pos, compress))
}

pub fn transcodings_list(user_agent: Option<&str>, compress: bool) -> ResponseResult {
    let transcodings = user_agent
        .map(Transcodings::for_user_agent)
        .unwrap_or_default();
    Ok(json_response(&transcodings, compress))
}

pub async fn search(
    collection: usize,
    searcher: Search<String>,
    query: String,
    ordering: FoldersOrdering,
    group: Option<String>,
    compress: bool,
) -> ResponseResult {
    blocking(move || {
        let res = searcher.search(collection, query, ordering, group);
        json_response(&res, compress)
    })
    .await
    .map_err(Error::new)
}

pub async fn recent(
    collection: usize,
    searcher: Search<String>,
    group: Option<String>,
    compress: bool,
) -> ResponseResult {
    blocking(move || {
        let res = searcher.recent(collection, group);
        json_response(&res, compress)
    })
    .await
    .map_err(Error::new)
}
