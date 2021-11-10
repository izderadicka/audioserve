use std::collections::BinaryHeap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::audio_folder::FolderLister;
use crate::audio_meta::AudioFolder;
use crate::common::{CollectionTrait, PositionsData, PositionsTrait};
use crate::error::{Error, Result};
use crate::position::PositionsCollector;
use crate::util::get_real_file_type;
use crate::AudioFolderShort;

pub(crate) struct CollectionDirect {
    lister: FolderLister,
    base_dir: PathBuf,
    searcher: FoldersSearch,
}

impl CollectionDirect {
    pub(crate) fn new(base_dir: PathBuf, lister: FolderLister, allow_symlinks: bool) -> Self {
        CollectionDirect {
            base_dir,
            lister,
            searcher: FoldersSearch { allow_symlinks },
        }
    }
}

impl CollectionTrait for CollectionDirect {
    fn list_dir<P>(
        &self,
        dir_path: P,
        ordering: crate::FoldersOrdering,
        _group: Option<String>,
    ) -> Result<AudioFolder>
    where
        P: AsRef<std::path::Path>,
    {
        self.lister
            .list_dir(&self.base_dir, dir_path, ordering)
            .map_err(Error::from)
    }

    fn flush(&self) -> Result<()> {
        Ok(())
    }

    fn search<S: AsRef<str>>(&self, q: S) -> Vec<crate::AudioFolderShort> {
        self.searcher.search_folder(&self.base_dir, q)
    }

    fn recent(&self, limit: usize) -> Vec<crate::AudioFolderShort> {
        self.searcher
            .search_folder_for_recent(&self.base_dir, limit)
    }

    fn signal_rescan(&self) {}

    fn base_dir(&self) -> &Path {
        self.base_dir.as_path()
    }
}

impl PositionsTrait for CollectionDirect {
    fn insert_position<S, P>(
        &self,
        _group: S,
        _path: P,
        _position: f32,
        _folder_finished: bool,
        _ts: Option<crate::audio_meta::TimeStamp>,
    ) -> Result<()>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        Ok(())
    }

    fn get_position<S, P>(&self, _group: S, _folder: Option<P>) -> Option<crate::Position>
    where
        S: AsRef<str>,
        P: AsRef<str>,
    {
        None
    }

    fn get_all_positions_for_group<S>(
        &self,
        _group: S,
        _collection_no: usize,
        _res: &mut PositionsCollector,
    ) where
        S: AsRef<str>,
    {
    }

    fn write_json_positions<F: std::io::Write>(&self, _file: &mut F) -> Result<()> {
        Ok(())
    }

    fn read_json_positions(&self, _data: PositionsData) -> Result<()> {
        Ok(())
    }

    fn get_positions_recursive<S, P>(
        &self,
        _group: S,
        _folder: P,
        _collection_no: usize,
        _res: &mut PositionsCollector,
    ) where
        S: AsRef<str>,
        P: AsRef<str>,
    {
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

struct FoldersSearch {
    allow_symlinks: bool,
}

impl FoldersSearch {
    fn search_folder_for_recent<P: AsRef<Path>>(
        &self,
        base_dir: P,
        limit: usize,
    ) -> Vec<AudioFolderShort> {
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
        }
        let base_path = base_dir.as_ref();
        let allow_symlinks = self.allow_symlinks;
        search_recursive(base_path, base_path, &mut recents, allow_symlinks, limit);
        let dirs = recents.into_sorted_vec();
        dirs.into_iter()
            .map(|e| AudioFolderShort::from_path(base_path, e.path))
            .collect()
    }

    fn search_folder<P: AsRef<Path>, S: AsRef<str>>(
        &self,
        base_dir: P,
        query: S,
    ) -> Vec<AudioFolderShort> {
        fn search_recursive(
            base_path: &Path,
            path: &Path,
            results: &mut Vec<AudioFolderShort>,
            tokens: &[String],
            allow_symlinks: bool,
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
                                        let folder =
                                            AudioFolderShort::from_dir_entry(&f, s.into(), false);
                                        if let Ok(folder) = folder {
                                            results.push(folder)
                                        }
                                    } else {
                                        search_recursive(
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

        let tokens: Vec<String> = query
            .as_ref()
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();
        let mut res = Vec::new();
        search_recursive(
            base_dir.as_ref(),
            base_dir.as_ref(),
            &mut res,
            &tokens,
            self.allow_symlinks,
        );
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DATA_DIR: &str = "../../test_data";

    #[test]
    fn test_search_folders() {
        let search = FoldersSearch {
            allow_symlinks: false,
        };
        let res = search.search_folder(TEST_DATA_DIR, "usak kulisak");
        assert_eq!(res.len(), 1);

        let res = search.search_folder(TEST_DATA_DIR, "usak nexistuje");
        assert_eq!(res.len(), 0);

        let res = search.search_folder(TEST_DATA_DIR, "t");
        assert_eq!(res.len(), 0);
    }

    #[test]
    fn test_recents() {
        let search = FoldersSearch {
            allow_symlinks: false,
        };
        let res = search.search_folder_for_recent(TEST_DATA_DIR, 100);
        assert_eq!(2, res.len());
        let times = res
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
