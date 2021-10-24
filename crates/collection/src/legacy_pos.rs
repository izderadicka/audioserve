use std::collections::HashMap;

use serde_derive::Deserialize;

use crate::audio_meta::TimeStamp;

#[derive(Clone, Deserialize)]
pub(super) struct LegacyTimestamp {
    pub secs_since_epoch: u64,
    pub nanos_since_epoch: u64,
}

impl From<LegacyTimestamp> for TimeStamp {
    fn from(ts: LegacyTimestamp) -> Self {
        let ms = ts.secs_since_epoch * 1000 + ts.nanos_since_epoch / 1_000_000;
        ms.into()
    }
}

#[derive(Clone, Deserialize)]
pub(super) struct LegacyPosition {
    pub file: String,
    pub position: f32,
    pub timestamp: LegacyTimestamp,
}

#[derive(Clone, Deserialize)]
pub(super) struct LegacyPositions {
    pub table: HashMap<String, HashMap<String, LegacyPosition>>,
    pub max_size: usize,
    pub max_groups: usize,
}
