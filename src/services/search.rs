use super::audio_folder::get_real_file_type;
use super::types::{AudioFolderShort, FoldersOrdering, SearchResult};
use crate::config::get_config;
use std::collections::BinaryHeap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

const RECENT_LIST_SIZE: usize = 64;

pub trait SearchTrait<S: AsRef<str>> {
    fn search(&self, collection: usize, query: S, ordering: FoldersOrdering) -> SearchResult;
    fn recent(&self, collection: usize) -> SearchResult;
}

struct FoldersSearch;

#[derive(Clone)]
pub struct Search<S: AsRef<str>> {
    inner: Arc<Box<SearchTrait<S> + Send + Sync>>,
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
    #[cfg(feature = "search-cache")]
    pub fn new() -> Self {
        if get_config().search_cache {
            info!("Using search cache");
            Search {
                inner: Arc::new(Box::new(cache::CachedSearch::new())),
            }
        } else {
            Search {
                inner: Arc::new(Box::new(FoldersSearch)),
            }
        }
    }

    #[cfg(not(feature = "search-cache"))]
    pub fn new() -> Self {
        Search {
            inner: Arc::new(Box::new(FoldersSearch)),
        }
    }
}

impl<S: AsRef<str>> SearchTrait<S> for FoldersSearch {
    fn search(&self, collection: usize, query: S, ordering: FoldersOrdering) -> SearchResult {
        self.search_folder(&get_config().base_dirs[collection], query, ordering)
    }

    fn recent(&self, collection: usize) -> SearchResult {
        self.search_folder_for_recent(&get_config().base_dirs[collection], RECENT_LIST_SIZE)
    }
}

#[derive(PartialEq, Eq)]
struct DirEntry {
    path: PathBuf,
    created: SystemTime,
}

// need reverse ordering for heap, oldest will be on top
use std::cmp::Ordering;
impl PartialOrd for DirEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(match self.created.cmp(&other.created) {
            Ordering::Greater => Ordering::Less,
            Ordering::Less => Ordering::Greater,
            Ordering::Equal => self.path.cmp(&other.path),
        })
    }
}
impl Ord for DirEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(&other).unwrap()
    }
}

impl FoldersSearch {
    fn search_folder_for_recent<P: AsRef<Path>>(&self, base_dir: P, limit: usize) -> SearchResult {
        let mut res = SearchResult::new();
        let mut recents: BinaryHeap<DirEntry> = BinaryHeap::with_capacity(limit);

        fn search_recursive(
            base_path: &Path,
            path: &Path,
            res: &mut BinaryHeap<DirEntry>,
            allow_symlinks: bool,
            limit: usize,
        ) {
            if let Ok(dir_iter) = fs::read_dir(path) {
                for item in dir_iter {
                    if let Ok(f) = item {
                        if let Ok(ft) = get_real_file_type(&f, path, allow_symlinks) {
                            if ft.is_dir() {
                                let p = f.path();
                                search_recursive(base_path, &p, res, allow_symlinks, limit);
                                if let Ok(meta) = p.metadata() {
                                    let changed = meta.modified();

                                    if let Ok(changed) = changed {
                                        if res.len() >= limit {
                                            res.pop();
                                        }
                                        res.push(DirEntry {
                                            path: p,
                                            created: changed,
                                        })
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };
        let base_path = base_dir.as_ref();
        let allow_symlinks = get_config().allow_symlinks;
        search_recursive(base_path, base_path, &mut recents, allow_symlinks, limit);
        let dirs = recents.into_sorted_vec();
        res.subfolders.extend(
            dirs.into_iter()
                .map(|e| AudioFolderShort::from_path(base_path, e.path)),
        );
        res
    }

    fn search_folder<P: AsRef<Path>, S: AsRef<str>>(
        &self,
        base_dir: P,
        query: S,
        ordering: FoldersOrdering,
    ) -> SearchResult {
        fn search_recursive(
            base_path: &Path,
            path: &Path,
            results: &mut SearchResult,
            tokens: &[String],
            allow_symlinks: bool,
            ordering: FoldersOrdering,
        ) {
            if let Ok(dir_iter) = fs::read_dir(path) {
                for item in dir_iter {
                    if let Ok(f) = item {
                        if let Ok(ft) = get_real_file_type(&f, path, allow_symlinks) {
                            if ft.is_dir() {
                                let p = f.path();
                                if let Some(s) =
                                    p.strip_prefix(base_path).ok().and_then(Path::to_str)
                                {
                                    let lc_s = s.to_lowercase();
                                    let m = tokens.iter().all(|token| lc_s.contains(token));
                                    if m {
                                        debug!("Found {:?} in {}", tokens, lc_s);
                                        let folder = AudioFolderShort::from_dir_entry(
                                            &f,
                                            s.into(),
                                            ordering,
                                            false,
                                        );
                                        if let Ok(folder) = folder {
                                            results.subfolders.push(folder)
                                        }
                                    } else {
                                        search_recursive(
                                            base_path,
                                            &p,
                                            results,
                                            tokens,
                                            allow_symlinks,
                                            ordering,
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let tokens: Vec<String> = query
            .as_ref()
            .split(' ')
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();
        let mut res = SearchResult::new();
        search_recursive(
            base_dir.as_ref(),
            base_dir.as_ref(),
            &mut res,
            &tokens,
            get_config().allow_symlinks,
            ordering,
        );
        res.subfolders
            .sort_unstable_by(|a, b| a.compare_as(ordering, b));
        res
    }
}

#[cfg(feature = "search-cache")]
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
                            s.modified = Some(modified)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::init::init_default_config;

    const TEST_DATA_DIR: &str = "./test_data";

    #[test]
    fn test_search_folders() {
        init_default_config();
        let search = FoldersSearch;
        let res = search.search_folder(TEST_DATA_DIR, "usak kulisak", FoldersOrdering::RecentFirst);
        assert_eq!(res.subfolders.len(), 1);

        let res = search.search_folder(
            TEST_DATA_DIR,
            "usak nexistuje",
            FoldersOrdering::RecentFirst,
        );
        assert_eq!(res.subfolders.len(), 0);

        let res = search.search_folder(TEST_DATA_DIR, "t", FoldersOrdering::RecentFirst);
        assert_eq!(res.subfolders.len(), 0);
    }

    #[test]
    fn test_recents() {
        init_default_config();
        let search = FoldersSearch;
        let res = search.search_folder_for_recent(TEST_DATA_DIR, 100);
        assert_eq!(2, res.subfolders.len());
        let times = res
            .subfolders
            .into_iter()
            .map(|p| {
                let path = Path::new(TEST_DATA_DIR).join(p.path);
                let meta = path.metadata().unwrap();
                meta.modified().unwrap()
            })
            .collect::<Vec<_>>();
        assert!(times[0] >= times[1]);
    }
}
