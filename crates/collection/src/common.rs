use crate::{
    audio_meta::{AudioFolder, TimeStamp},
    cache::{CollectionCache, Search},
    error::Result,
    AudioFolderShort, FoldersOrdering, Position,
};
use enum_dispatch::enum_dispatch;
use std::path::Path;

#[enum_dispatch(CollectionTrait)]
#[enum_dispatch(PositionsTrait)]
pub(crate) enum Collection {
    CollectionCache,
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
}

#[cfg(feature = "async")]
impl Collection {
    pub async fn insert_position_async<S, P>(
        &self,
        group: S,
        path: P,
        position: f32,
        ts: Option<TimeStamp>,
    ) -> Result<()>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        match self {
            Collection::CollectionCache(inner) => {
                inner.insert_position_async(group, path, position, ts).await
            }
        }
    }

    pub async fn get_position_async<S, P>(&self, group: S, path: Option<P>) -> Option<Position>
    where
        S: AsRef<str> + Send + 'static,
        P: AsRef<str> + Send + 'static,
    {
        match self {
            Collection::CollectionCache(inner) => inner.get_position_async(group, path).await,
        }
    }
}

#[enum_dispatch]
pub(crate) trait CollectionTrait {
    fn list_dir<P>(&self, dir_path: P, ordering: FoldersOrdering) -> Result<AudioFolder>
    where
        P: AsRef<Path>;

    fn flush(&self) -> Result<()>;

    fn search<S: AsRef<str>>(&self, q: S) -> Search;

    fn recent(&self, limit: usize) -> Vec<AudioFolderShort>;
}

// impl Collection {

//     fn list_dir<P>(&self, dir_path: P, ordering: FoldersOrdering) -> Result<AudioFolder>
//         where P: AsRef<Path>
//     {
//        match self {
//            Collection::Cached(col) => col.list_dir(dir_path, ordering)
//        }
//     }
//     }
