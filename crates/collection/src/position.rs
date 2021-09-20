use serde_derive::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

use crate::audio_meta::TimeStamp;

pub const MAX_GROUPS: usize = 100;
pub const MAX_HISTORY_PER_FOLDER: usize = 10;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct PositionItem {
    file: String,
    timestamp: TimeStamp,
    position: f32,
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
    file: String,
    folder: String,
    timestamp: TimeStamp,
    position: f32,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PositionRecord {
    latest: Position,
    folder_positions: HashMap<String, VecDeque<PositionItem>>,
}
