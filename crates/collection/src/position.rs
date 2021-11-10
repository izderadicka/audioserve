use serde_derive::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
};

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

#[derive(Clone, Serialize, Deserialize, PartialEq, PartialOrd, Debug)]
pub struct Position {
    pub timestamp: TimeStamp,
    pub collection: usize,
    pub folder: String,
    pub file: String,
    #[serde(default)]
    pub folder_finished: bool,
    pub position: f32,
}

impl Eq for Position {}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.partial_cmp(other) {
            Some(o) => o,
            //  None can be only if everything is equal, but position contains f32::NAN
            // In this can choose arbitrary inequality, as eq is false
            None => std::cmp::Ordering::Greater,
        }
    }
}

pub(crate) type PositionRecord = HashMap<String, PositionItem>;

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct PositionShort {
    pub path: String,
    pub timestamp: TimeStamp,
    pub position: f32,
}

pub(crate) type PositionsCollector = Collector<Position>;

pub(crate) struct Collector<T> {
    heap: BinaryHeap<Reverse<T>>,
    max_size: usize,
}

impl<T: Ord> Collector<T> {
    pub(crate) fn new(max_size: usize) -> Self {
        Collector {
            heap: BinaryHeap::new(),
            max_size,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn with_capacity(max_size: usize, capacity: usize) -> Self {
        Collector {
            heap: BinaryHeap::with_capacity(capacity),
            max_size,
        }
    }

    pub(crate) fn add(&mut self, item: T) {
        self.heap.push(Reverse(item));
        if self.heap.len() > self.max_size {
            self.heap.pop();
        }
    }

    pub(crate) fn into_vec(self) -> Vec<T> {
        let v = self.heap.into_sorted_vec();
        v.into_iter().map(|i| i.0).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector() {
        let data = vec![1, 7, 5, 9, 0, 8, 3, 2, 4, 6];
        let mut c = Collector::with_capacity(4, 5);
        data.iter().for_each(|i| c.add(*i));
        let res = c.into_vec();
        assert_eq!(vec![9, 8, 7, 6], res);
    }
}
