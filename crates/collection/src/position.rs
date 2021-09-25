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
    pub(crate) fn to_position<S: Into<String>>(self, folder: S) -> Position {
        Position {
            file: self.file,
            folder: folder.into(),
            timestamp: self.timestamp,
            position: self.position,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub file: String,
    pub folder: String,
    pub timestamp: TimeStamp,
    pub position: f32,
}

pub(crate) type PositionRecord = HashMap<String, PositionItem>;
