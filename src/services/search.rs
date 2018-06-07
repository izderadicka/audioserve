use super::get_real_file_type;
use super::types::{AudioFolderShort, SearchResult};
use std::fs;
use std::path::Path;
use config::get_config;

pub trait SearchTrait {
    fn search<P: AsRef<Path>, S: AsRef<str>>(&self, base_dir: P, query: S) -> SearchResult;
}

#[derive(Clone)]
struct FoldersSearch;

#[derive(Clone)]
pub enum Search {
    FoldersSearch,
}

//As we cannot use Trait object due to generic types, we can alternatively use enum and dispatch search fn here
impl SearchTrait for Search {
    fn search<P: AsRef<Path>, S: AsRef<str>>(&self, base_dir: P, query: S) -> SearchResult {
        match self {
            &Search::FoldersSearch => FoldersSearch.search(base_dir, query),
        }
    }
}

impl SearchTrait for FoldersSearch {
    fn search<P: AsRef<Path>, S: AsRef<str>>(&self, base_dir: P, query: S) -> SearchResult {
        let tokens: Vec<String> = query
            .as_ref()
            .split(" ")
            .filter(|s| s.len() > 0)
            .map(|s| s.to_lowercase())
            .collect();
        let mut res = SearchResult {
            subfolders: vec![],
            files: vec![],
        };
        FoldersSearch::search_recursive(base_dir.as_ref(), base_dir.as_ref(), &mut res, &tokens, 
            get_config().allow_symlinks);
        res
    }
}

impl FoldersSearch {
    fn search_recursive(
        base_path: &Path,
        path: &Path,
        results: &mut SearchResult,
        tokens: &Vec<String>,
        allow_symlinks: bool
    ) {
        if let Ok(dir_iter) = fs::read_dir(path) {
            for item in dir_iter {
                if let Ok(f) = item {
                    if let Ok(ft) = get_real_file_type(&f, path, allow_symlinks) {
                        if ft.is_dir() {
                            let p = f.path();
                            if let Ok(s) = p.clone().into_os_string().into_string() {
                                let lc_s = s.to_lowercase();
                                let m = tokens.into_iter().all(|token| lc_s.contains(token));
                                if m {
                                    results.subfolders.push(AudioFolderShort {
                                        name: p.file_name().unwrap().to_str().unwrap().into(),
                                        path: p.strip_prefix(base_path).unwrap().into(),
                                    })
                                } else {
                                    FoldersSearch::search_recursive(base_path, &p, results, tokens, allow_symlinks)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_folders() {
        let search = FoldersSearch;
        let res = search.search("./test_data", "usak kulisak");
        assert_eq!(res.subfolders.len(), 1);

        let res = search.search("./test_data", "usak nexistuje");
        assert_eq!(res.subfolders.len(), 0);
    }
}
