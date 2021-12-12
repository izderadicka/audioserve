use crate::{
    audio_folder::FolderOptions,
    audio_meta::{AudioFolder, TimeStamp},
    cache::CollectionCache,
    error::{invalid_option, invalid_option_err, Result},
    no_cache::CollectionDirect,
    position::PositionsCollector,
    AudioFolderShort, FoldersOrdering, Position,
};
use enum_dispatch::enum_dispatch;
use media_info::tags::{ALLOWED_TAGS, BASIC_TAGS};
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Minimum chapter duration for splitting - in minutes
pub const MINIMUM_CHAPTER_DURATION: u32 = 10;

pub enum PositionsData {
    Legacy(()),
    V1(Map<String, Value>),
}

#[derive(Clone)]
pub struct CollectionOptions {
    pub no_cache: bool,
    pub folder_options: FolderOptions,
    pub force_cache_update_on_init: bool,
}

impl Default for CollectionOptions {
    fn default() -> Self {
        Self {
            no_cache: false,
            folder_options: Default::default(),
            force_cache_update_on_init: false,
        }
    }
}

impl CollectionOptions {
    pub fn update_from_str_options(&mut self, s: &str) -> Result<()> {
        let options = s.split(',');
        for option in options {
            let mut expr_iter = option.splitn(2, '=').map(|s| s.trim());
            if let Some(tag) = expr_iter.next() {
                let val = expr_iter.next();
                let bool_val = || {
                    val.map(|s| match s.to_ascii_lowercase().as_str() {
                        "true" => Ok(true),
                        "false" => Ok(false),
                        _ => invalid_option!("Invalid value {} for option {}", s, tag),
                    })
                    .unwrap_or(Ok(true))
                };

                let u32_val = || {
                    val.map(|s| {
                        s.parse::<u32>().map_err(|_| {
                            invalid_option_err!("NonInteger value {} for option {}", s, tag)
                        })
                    })
                    .unwrap_or_else(|| invalid_option!("Value is required for option: {}", tag))
                };
                match tag {
                    "nc" | "no-cache" => self.no_cache = bool_val()?,
                    "force-cache-update" => self.force_cache_update_on_init = bool_val()?,
                    "ignore-chapters-meta" => {
                        self.folder_options.ignore_chapters_meta = bool_val()?
                    }
                    "allow-symlinks" => self.folder_options.allow_symlinks = bool_val()?,
                    "no-dir-collaps" => self.folder_options.no_dir_collaps = bool_val()?,
                    "chapters-duration" => {
                        let val = u32_val()?;
                        if val < MINIMUM_CHAPTER_DURATION {
                            invalid_option!("Option {} has invalid value - value {} is below limit for reasonable chapter size", tag, val);
                        }
                        self.folder_options.chapters_duration = val;
                    }
                    "chapters-from-duration" => {
                        let val = u32_val()?;
                        if val > 0 && val < MINIMUM_CHAPTER_DURATION {
                            invalid_option!("Option {} has invalid value - value {} is below limit for reasonable chapter size", tag, val);
                        }
                        self.folder_options.chapters_from_duration = val;
                    }
                    "tags" => {
                        if let Some(tags) = val {
                            let tags = tags
                                .split('+')
                                .map(|s| s.trim().to_ascii_lowercase())
                                .map(|s| {
                                    if ALLOWED_TAGS.contains(&s.as_str()) {
                                        Ok(s)
                                    } else {
                                        invalid_option!("This tag {} is not allowed", s);
                                    }
                                })
                                .collect::<Result<HashSet<_>>>()?;
                            self.folder_options.tags = Some(tags);
                        } else {
                            invalid_option!("Some tags are required for {}", tag);
                        }
                    }
                    "default_tags" => {
                        if bool_val()? {
                            self.folder_options.tags =
                                Some(BASIC_TAGS.iter().map(|i| i.to_string()).collect())
                        } else {
                            self.folder_options.tags = None
                        }
                    }

                    opt => invalid_option!("Unknown option: {}", opt),
                }
            }
        }

        Ok(())
    }
}

pub struct CollectionOptionsMap {
    cols: HashMap<PathBuf, CollectionOptions>,
    default: CollectionOptions,
}

impl CollectionOptionsMap {
    pub fn new(default_folder_options: FolderOptions, force_cache_update: bool) -> Self {
        let mut default = CollectionOptions::default();
        default.force_cache_update_on_init = force_cache_update;
        default.folder_options = default_folder_options;
        CollectionOptionsMap {
            cols: HashMap::new(),
            default,
        }
    }

    pub fn add_col_options(&mut self, path: impl Into<PathBuf>, col_options: &str) -> Result<()> {
        let mut col_opt = self.default.clone();
        col_opt.update_from_str_options(col_options)?;
        self.cols.insert(path.into(), col_opt);
        Ok(())
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_col_options() {
        let mut opt = CollectionOptions::default();
        opt.update_from_str_options("no-cache,force-cache-update=true,ignore-chapters-meta=false,allow-symlinks,no-dir-collaps=TRUE").expect("good options");
        assert!(opt.no_cache);
        assert!(opt.force_cache_update_on_init);
        assert!(!opt.folder_options.ignore_chapters_meta);
        assert!(opt.folder_options.allow_symlinks);
        assert!(opt.folder_options.no_dir_collaps);

        opt.update_from_str_options("tags=title+album+composer")
            .expect("valid tags");
        assert_eq!(3, opt.folder_options.tags.as_ref().unwrap().len());

        opt.update_from_str_options("chapters-duration=44,chapters-from-duration=200")
            .expect("correct options");
        assert_eq!(44, opt.folder_options.chapters_duration);
        assert_eq!(200, opt.folder_options.chapters_from_duration);
    }
}
