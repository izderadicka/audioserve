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

pub(crate) type PositionsCollector = Collector<Position, PositionFilter>;

pub struct PositionFilter {
    finished: Option<bool>,
    from: Option<TimeStamp>,
    to: Option<TimeStamp>,
}

impl PositionFilter {
    pub fn new(finished: Option<bool>, from: Option<TimeStamp>, to: Option<TimeStamp>) -> Self {
        Self { finished, from, to }
    }

    pub(crate) fn into_option(self) -> Option<Self> {
        if self.finished.is_none() && self.from.is_none() && self.to.is_none() {
            None
        } else {
            Some(self)
        }
    }
}

impl CollectorFilter<Position> for PositionFilter {
    fn filter(&self, item: &Position) -> bool {
        let finished = self
            .finished
            .as_ref()
            .map(|finished| *finished == item.folder_finished)
            .unwrap_or(true);

        let before = self
            .to
            .as_ref()
            .map(|before| item.timestamp < *before)
            .unwrap_or(true);

        let after = self
            .from
            .as_ref()
            .map(|after| item.timestamp >= *after)
            .unwrap_or(true);

        finished && before && after
    }
}

pub(crate) trait CollectorFilter<T> {
    fn filter(&self, item: &T) -> bool;
}

impl<F, T> CollectorFilter<T> for F
where
    F: Fn(&T) -> bool,
{
    fn filter(&self, item: &T) -> bool {
        self(item)
    }
}
pub(crate) struct Collector<T, F> {
    heap: BinaryHeap<Reverse<T>>,
    max_size: usize,
    filter: Option<F>,
}

impl<T, F> Collector<T, F>
where
    T: Ord,
    F: CollectorFilter<T>,
{
    #[allow(dead_code)]
    pub(crate) fn new(max_size: usize) -> Self {
        Collector {
            heap: BinaryHeap::new(),
            max_size,
            filter: None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn with_capacity_and_filter(max_size: usize, capacity: usize, filter: F) -> Self {
        Collector {
            heap: BinaryHeap::with_capacity(capacity),
            max_size,
            filter: Some(filter),
        }
    }

    pub(crate) fn with_optional_filter(max_size: usize, filter: Option<F>) -> Self {
        Collector {
            heap: BinaryHeap::new(),
            max_size,
            filter: filter,
        }
    }

    pub(crate) fn add(&mut self, item: T) {
        if self
            .filter
            .as_ref()
            .map(|f| f.filter(&item))
            .unwrap_or(true)
        {
            self.heap.push(Reverse(item));
            if self.heap.len() > self.max_size {
                self.heap.pop();
            }
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
        let data: Vec<i32> = vec![1, 7, 5, 9, 0, 8, 3, 2, 4, 6];
        let mut c = Collector::with_capacity_and_filter(4, 5, |i: &i32| *i != 7);
        data.iter().for_each(|i| c.add(*i));
        let res = c.into_vec();
        assert_eq!(vec![9, 8, 6, 5], res);
    }
}
