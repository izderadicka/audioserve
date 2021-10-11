//TODO: Legacy search cache implementation
// In case people liked old ways
// Could be implemented as another Collection

#[cfg(feature = "legacy-cache")]
mod cache {
    use super::*;
    use cachedirtree::{DirCache, OptionsBuilder};

    pub struct CachedSearch {
        caches: Vec<DirCache>,
    }

    impl CachedSearch {
        pub fn new() -> Self {
            let opts = OptionsBuilder::default()
                .include_files(false)
                .watch_changes(true)
                .follow_symlinks(get_config().allow_symlinks)
                .recent_list_size(RECENT_LIST_SIZE)
                .build()
                .unwrap();
            let caches = get_config()
                .base_dirs
                .iter()
                .map(|p| DirCache::new_with_options(p, opts))
                .collect();

            CachedSearch { caches }
        }
    }

    impl<S: AsRef<str>> SearchTrait<S> for CachedSearch {
        fn search(&self, collection: usize, query: S, ordering: FoldersOrdering) -> SearchResult {
            let mut res = self.caches[collection]
                .search_collected(query, |iter| {
                    let mut res = SearchResult::new();
                    iter.for_each(|e| {
                        res.subfolders.push(AudioFolderShort::from_path_and_name(
                            e.name(),
                            e.path(),
                            false,
                        ))
                    });
                    res
                })
                .map_err(|e| error!("Search failed {}", e))
                .unwrap_or_else(|_| SearchResult::new());

            // As search cache now does not contain modified times we need to add them here
            // This is kind of hack, but as this is probably not common I guess it's easier
            // then adding mtime into search cache
            if let FoldersOrdering::RecentFirst = ordering {
                let base_path = &get_config().base_dirs[collection];
                //need to update mtime
                res.subfolders.iter_mut().for_each(|s| {
                    let full_path = base_path.join(&s.path);
                    if let Ok(metadata) = fs::metadata(full_path) {
                        if let Ok(modified) = metadata.modified() {
                            s.modified = Some(modified.into())
                        }
                    }
                });
            };
            res.subfolders
                .sort_unstable_by(|a, b| a.compare_as(ordering, b));
            res
        }

        fn recent(&self, collection: usize) -> SearchResult {
            let mut res = SearchResult::new();

            self.caches[collection]
                .recent()
                .map(|v| {
                    let subfolders = v
                        .into_iter()
                        .map(|p| AudioFolderShort::from_path(Path::new(""), p))
                        .collect();
                    res.subfolders = subfolders;
                })
                .map_err(|e| error!("Recents failed {}", e))
                .ok();

            res
        }
    }
}
