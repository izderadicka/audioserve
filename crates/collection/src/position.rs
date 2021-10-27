use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::audio_meta::TimeStamp;

pub const MAX_GROUPS: usize = 100;
pub const MAX_HISTORY_PER_FOLDER: usize = 10;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PositionItem {
    pub file: String,
    pub timestamp: TimeStamp,
    pub position: f32,
    pub folder_finished: bool,
}

impl PositionItem {
    pub(crate) fn into_position<S: Into<String>>(&self, folder: S, collection: usize) -> Position {
        Position {
            file: self.file.clone(),
            folder: folder.into(),
            timestamp: self.timestamp,
            position: self.position,
            collection,
            folder_finished: self.folder_finished,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Position {
    pub file: String,
    pub folder: String,
    pub timestamp: TimeStamp,
    pub position: f32,
    pub collection: usize,
    #[serde(default)]
    pub folder_finished: bool,
}

pub(crate) type PositionRecord = HashMap<String, PositionItem>;

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct PositionShort {
    pub path: String,
    pub timestamp: TimeStamp,
    pub position: f32,
}
