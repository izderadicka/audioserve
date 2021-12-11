use crate::{
    audio_folder::FolderOptions,
    audio_meta::{AudioFolder, TimeStamp},
    cache::CollectionCache,
    error::Result,
    no_cache::CollectionDirect,
    position::PositionsCollector,
    AudioFolderShort, FoldersOrdering, Position, VERSION,
};
use enum_dispatch::enum_dispatch;
use serde_derive::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub enum PositionsData {
    Legacy(()),
    V1(Map<String, Value>),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CollectionOptions {
    pub no_cache: bool,
    pub folder_options: FolderOptions,
    pub col_version: &'static str,
    pub pgm_version: &'static str,
    pub force_cache_update_on_init: bool,
}

impl Default for CollectionOptions {
    fn default() -> Self {
        Self {
            no_cache: false,
            folder_options: Default::default(),
            col_version: VERSION,
            pgm_version: Default::default(),
            force_cache_update_on_init: false,
        }
    }
}

pub struct CollectionOptionsMap {
    cols: HashMap<PathBuf, CollectionOptions>,
    default: CollectionOptions,
}

impl CollectionOptionsMap {
    pub fn new(
        default_folder_options: FolderOptions,
        force_cache_update: bool,
        pgm_version: &'static str,
    ) -> Self {
        let mut default = CollectionOptions::default();
        default.force_cache_update_on_init = force_cache_update;
        default.folder_options = default_folder_options;
        default.pgm_version = pgm_version;
        CollectionOptionsMap {
            cols: HashMap::new(),
            default,
        }
    }

    pub fn add_col_options(&mut self, path: impl Into<PathBuf>, col_options: ()) {}

    pub fn get_col_options(&mut self, path: impl AsRef<Path>) -> CollectionOptions {
        self.cols
            .remove(path.as_ref())
            .unwrap_or_else(|| self.default.clone())
    }
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
        finished: bool,
        ts: Option<TimeStamp>,
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>;

    fn get_position<S, P>(&self, group: S, folder: Option<P>) -> Option<Position>
    where
        S: AsRef<str>,
        P: AsRef<str>;

    fn get_positions_recursive<S, P>(
        &self,
        group: S,
        folder: P,
        collection_no: usize,
        res: &mut PositionsCollector,
    ) where
        S: AsRef<str>,
        P: AsRef<str>;

    fn get_all_positions_for_group<S>(
        &self,
        group: S,
        collection_no: usize,
        res: &mut PositionsCollector,
    ) where
        S: AsRef<str>;

    fn write_json_positions<F: std::io::Write>(&self, file: &mut F) -> Result<()>;

    fn read_json_positions(&self, data: PositionsData) -> Result<()>;
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

    fn base_dir(&self) -> &Path;
}
