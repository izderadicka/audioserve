use super::get_real_file_type;
use super::types::{AudioFolderShort, SearchResult};
use config::get_config;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use cachedirtree::{DirCache, OptionsBuilder};


pub trait SearchTrait< S: AsRef<str>> {
    fn search(&self, collection: usize, query: S) -> SearchResult;
}


struct FoldersSearch;

#[derive(Clone)]
pub struct Search< S: AsRef<str>> {
    inner: Arc<Box<SearchTrait<S>+Send+Sync>>

}

impl < S: AsRef<str>> SearchTrait<S> for Search<S> {
    fn search(&self, collection: usize, query: S) -> SearchResult {
        self.inner.search(collection, query)
    }
}

impl < S: AsRef<str>> Search<S> {
    pub fn new() -> Self {
        Search{
            inner: Arc::new(Box::new(CachedSearch::new()))
        }
    }
}

impl < S: AsRef<str>>SearchTrait<S> for FoldersSearch {
    fn search(&self, collection: usize, query: S) -> SearchResult {
        self.search_folder(&get_config().base_dirs[collection], query)
    }
}

impl FoldersSearch {

    fn search_folder<P: AsRef<Path>, S: AsRef<str>>(&self, base_dir: P, query: S) -> SearchResult {
        let tokens: Vec<String> = query
            .as_ref()
            .split(' ')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();
        let mut res = SearchResult::new();
        FoldersSearch::search_recursive(
            base_dir.as_ref(),
            base_dir.as_ref(),
            &mut res,
            &tokens,
            get_config().allow_symlinks,
        );
        res
    }
    

    fn search_recursive(
        base_path: &Path,
        path: &Path,
        results: &mut SearchResult,
        tokens: &[String],
        allow_symlinks: bool,
    ) {
        if let Ok(dir_iter) = fs::read_dir(path) {
            for item in dir_iter {
                if let Ok(f) = item {
                    if let Ok(ft) = get_real_file_type(&f, path, allow_symlinks) {
                        if ft.is_dir() {
                            let p = f.path();
                            if let Some(s) = p.to_str() {
                                let lc_s = s.to_lowercase();
                                let m = tokens.into_iter().all(|token| lc_s.contains(token));
                                if m {
                                    results.subfolders.push(AudioFolderShort {
                                        name: p.file_name().unwrap().to_str().unwrap().into(),
                                        path: p.strip_prefix(base_path).unwrap().into(),
                                    })
                                } else {
                                    FoldersSearch::search_recursive(
                                        base_path,
                                        &p,
                                        results,
                                        tokens,
                                        allow_symlinks,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

struct CachedSearch {
    caches: Vec<DirCache>
}

impl CachedSearch {
    fn new() -> Self {
        let opts = OptionsBuilder::default()
        .include_files(false)
        .watch_changes(true)
        .follow_symlinks(get_config().allow_symlinks)
        .build()
        .unwrap();
        let caches = get_config().base_dirs.iter()
            .map(|p| DirCache::new_with_options(p, opts))
            .collect();

        CachedSearch{caches}
    }
}

impl <S: AsRef<str>> SearchTrait<S> for CachedSearch {
    fn search(&self, collection: usize, query: S) -> SearchResult{
        self.caches[collection].search_collected(query, |iter|{
            let mut res =  SearchResult::new();
            iter.for_each(|e| {
                res.subfolders.push(
                    AudioFolderShort {
                        name: e.name(),
                        path: e.path(),
                                    }
                )
            });
            res
        })
        .map_err(|e| error!("Search failed {}",e))
        .unwrap_or(SearchResult::new())
        


        
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::init_default_config;

    #[test]
    fn test_search_folders() {
        init_default_config();
        let search = FoldersSearch;
        let res = search.search_folder("./test_data", "usak kulisak");
        assert_eq!(res.subfolders.len(), 1);

        let res = search.search_folder("./test_data", "usak nexistuje");
        assert_eq!(res.subfolders.len(), 0);
    }
}
