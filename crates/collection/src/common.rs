use crate::{
    audio_meta::{AudioFolder, TimeStamp},
    cache::CollectionCache,
    error::Result,
    no_cache::CollectionDirect,
    AudioFolderShort, FoldersOrdering, Position,
};
use enum_dispatch::enum_dispatch;
use std::path::Path;

pub struct CollectionOptions {
    pub no_cache: bool,
}

#[enum_dispatch(CollectionTrait, PositionsTrait)]
pub(crate) enum Collection {
    CollectionCache,
    CollectionDirect,
}

#[enum_dispatch]
pub(crate) trait PositionsTrait {
    fn insert_position<S, P>(
        &self,
        group: S,
        path: P,
        position: f32,
        ts: Option<TimeStamp>,
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>;

    fn get_position<S, P>(&self, group: S, folder: Option<P>) -> Option<Position>
    where
        S: AsRef<str>,
        P: AsRef<str>;

    fn get_all_positions_for_group<S>(&self, group: S, collection_no: usize) -> Vec<Position>
    where
        S: AsRef<str>;
}

#[enum_dispatch]
pub(crate) trait CollectionTrait {
    fn list_dir<P>(
        &self,
        dir_path: P,
        ordering: FoldersOrdering,
        group: Option<String>,
    ) -> Result<AudioFolder>
    where
        P: AsRef<Path>;

    fn flush(&self) -> Result<()>;

    fn search<S: AsRef<str>>(&self, q: S) -> Vec<AudioFolderShort>;

    fn recent(&self, limit: usize) -> Vec<AudioFolderShort>;

    fn signal_rescan(&self);
}
