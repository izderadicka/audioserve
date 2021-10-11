use super::types::SearchResult;
use collection::FoldersOrdering;
use std::sync::Arc;

pub trait SearchTrait<S> {
    fn search(&self, collection: usize, query: S, ordering: FoldersOrdering) -> SearchResult;
    fn recent(&self, collection: usize) -> SearchResult;
}

#[derive(Clone)]
pub struct Search<S> {
    inner: Arc<Box<dyn SearchTrait<S> + Send + Sync>>,
}

impl<S: AsRef<str>> SearchTrait<S> for Search<S> {
    fn search(&self, collection: usize, query: S, ordering: FoldersOrdering) -> SearchResult {
        self.inner.search(collection, query, ordering)
    }
    fn recent(&self, collection: usize) -> SearchResult {
        self.inner.recent(collection)
    }
}

impl<S: AsRef<str>> Search<S> {
    pub fn new(collections: Option<Arc<collection::Collections>>) -> Self {
        Search {
            inner: Arc::new(Box::new(col_db::CollectionsSearch::new(
                collections.unwrap(),
            ))),
        }
    }
}

mod col_db {
    use collection::Collections;

    use super::*;

    pub struct CollectionsSearch {
        collections: Arc<Collections>,
    }

    impl CollectionsSearch {
        pub fn new(collections: Arc<Collections>) -> Self {
            CollectionsSearch { collections }
        }
    }

    impl<T: AsRef<str>> SearchTrait<T> for CollectionsSearch {
        fn search(&self, collection: usize, query: T, ordering: FoldersOrdering) -> SearchResult {
            SearchResult {
                files: vec![],
                subfolders: self
                    .collections
                    .search(collection, query, ordering)
                    .map_err(|e| error!("Error in collections search: {}", e))
                    .unwrap_or_else(|_| vec![]),
            }
        }

        fn recent(&self, collection: usize) -> SearchResult {
            let res = self
                .collections
                .recent(collection, 100)
                .map_err(|e| error!("Cannot get recents from coolection db: {}", e))
                .unwrap_or_else(|_| vec![]);
            SearchResult {
                files: vec![],
                subfolders: res,
            }
        }
    }
}
