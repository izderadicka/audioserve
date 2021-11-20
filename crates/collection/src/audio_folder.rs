use std::borrow::{self, Cow};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::audio_meta::*;
use crate::util::{get_meta, get_modified, get_real_file_type, guess_mime_type};
use crate::collator::Collate;
use lazy_static::lazy_static;
use regex::Regex;

pub enum DirType {
    File {
        chapters: Vec<Chapter>,
        audio_meta: AudioMeta,
    },
    Dir,
    Other,
}

#[derive(Debug, Clone)]
pub struct FoldersOptions {
    pub chapters_duration: u32,
    pub chapters_from_duration: u32,
    pub ignore_chapters_meta: bool,
    pub allow_symlinks: bool,
    pub no_dir_collaps: bool,
    pub tags: Arc<Option<HashSet<String>>>,
    pub force_cache_update_on_init: bool,
}

impl Default for FoldersOptions {
    fn default() -> Self {
        Self {
            chapters_duration: 0,
            chapters_from_duration: 30,
            ignore_chapters_meta: false,
            allow_symlinks: false,
            no_dir_collaps: false,
            tags: Arc::new(None),
            force_cache_update_on_init: false,
        }
    }
}

#[derive(Clone)]
pub struct FolderLister {
    config: FoldersOptions,
}

impl FolderLister {
    pub fn new() -> Self {
        FolderLister {
            config: FoldersOptions::default(),
        }
    }

    pub fn new_with_options(config: FoldersOptions) -> Self {
        FolderLister { config }
    }
}

impl FolderLister {
    pub fn list_dir<P: AsRef<Path>, P2: AsRef<Path>>(
        &self,
        base_dir: P,
        dir_path: P2,
        ordering: FoldersOrdering,
    ) -> Result<AudioFolder, io::Error> {
        let full_path = base_dir.as_ref().join(&dir_path);
        match self.get_dir_type(&full_path)? {
            DirType::Dir => self.list_dir_dir(base_dir, full_path, ordering),
            DirType::File {
                chapters,
                audio_meta,
            } => self.list_dir_file(base_dir, full_path, audio_meta, chapters, false),
            DirType::Other => Err(io::Error::new(
                io::ErrorKind::Other,
                "Not folder or chapterised audio file",
            )),
        }
    }

    fn split_chapters(&self, dur: u32) -> Vec<Chapter> {
        let chap_length = u64::from(self.config.chapters_duration) * 60 * 1000;
        let mut count = 0;
        let mut start = 0u64;
        let tot = u64::from(dur) * 1000;
        let mut chaps = vec![];
        while start < tot {
            let end = start + chap_length;
            let dif: i64 = tot as i64 - end as i64;
            let end = if dif < chap_length as i64 / 3 {
                tot
            } else {
                end
            };
            chaps.push(Chapter {
                title: format!("Part {}", count),
                start,
                end,
                number: count,
            });
            count += 1;
            start = end;
        }
        chaps
    }

    pub fn get_dir_type<P: AsRef<Path>>(&self, path: P) -> Result<DirType, io::Error> {
        let path = path.as_ref();
        let meta = get_meta(path)?;
        if meta.is_dir() {
            Ok(DirType::Dir)
        } else if meta.is_file() && is_audio(path) {
            let meta =
                get_audio_properties(path).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            match (meta.get_chapters(), meta.get_audio_info(&self.config.tags)) {
                (Some(chapters), Some(audio_meta)) => Ok(DirType::File {
                    chapters,
                    audio_meta,
                }),
                (None, Some(audio_meta)) => match chapters_from_csv(path)? {
                    Some(chapters) => {
                        if chapters.len() > 1 {
                            Ok(DirType::File {
                                chapters,
                                audio_meta,
                            })
                        } else {
                            error!("Chapter file for {:?} has less then two chapters!", &path);
                            Ok(DirType::Other)
                        }
                    }
                    None => {
                        if self.is_long_file(Some(&audio_meta)) {
                            let chapters = self.split_chapters(audio_meta.duration);
                            Ok(DirType::File {
                                chapters,
                                audio_meta,
                            })
                        } else {
                            Ok(DirType::Other)
                        }
                    }
                },
                _ => Ok(DirType::Other),
            }
        } else {
            Ok(DirType::Other)
        }
    }

    fn is_long_file(&self, meta: Option<&AudioMeta>) -> bool {
        meta.map(|m| {
            let max_dur = self.config.chapters_from_duration * 60;
            max_dur > 60 * 10 && m.duration > max_dur
        })
        .unwrap_or(false)
    }

    fn list_dir_dir<P: AsRef<Path>>(
        &self,
        base_dir: P,
        full_path: PathBuf,
        ordering: FoldersOrdering,
    ) -> Result<AudioFolder, io::Error> {
        match fs::read_dir(&full_path) {
            Ok(dir_iter) => {
                let mut files = vec![];
                let mut subfolders = vec![];
                let mut cover = None;
                let mut description = None;
                let tags;
                let mut is_file = false;
                let allow_symlinks = self.config.allow_symlinks;

                for item in dir_iter {
                    match item {
                        Ok(f) => match get_real_file_type(&f, &full_path, allow_symlinks) {
                            Ok(ft) => {
                                let path = f.path().strip_prefix(&base_dir).unwrap().into();
                                if ft.is_dir() {
                                    subfolders
                                        .push(AudioFolderShort::from_dir_entry(&f, path, false)?)
                                } else if ft.is_file() {
                                    if is_audio(&path) {
                                        let mime = guess_mime_type(&path);
                                        let audio_file_path = base_dir.as_ref().join(&path);
                                        let meta = match get_audio_properties(&audio_file_path) {
                                            Ok(meta) => meta,
                                            Err(e) => {
                                                error!("Cannot add file {:?} because error in extraction audio meta: {}",path, e);
                                                continue;
                                            }
                                        };

                                        if !self.config.ignore_chapters_meta && meta.has_chapters()
                                        {
                                            // we do have chapters so let present this file as folder
                                            subfolders.push(AudioFolderShort::from_dir_entry(
                                                &f, path, true,
                                            )?)
                                        } else {
                                            let meta = meta.get_audio_info(&self.config.tags);
                                            if self.is_long_file((&meta).as_ref())
                                                || chapters_file_path(&audio_file_path)
                                                    .map(|p| p.is_file())
                                                    .unwrap_or(false)
                                            {
                                                // file is bigger then limit present as folder
                                                subfolders.push(AudioFolderShort::from_dir_entry(
                                                    &f, path, true,
                                                )?)
                                            } else {
                                                files.push(AudioFile {
                                                    meta,
                                                    path,
                                                    name: f.file_name().to_string_lossy().into(),
                                                    section: None,
                                                    mime: mime.to_string(),
                                                });
                                            }
                                        };
                                    } else if cover.is_none() && is_cover(&path) {
                                        cover = Some(TypedFile::new(path))
                                    } else if description.is_none() && is_description(&path) {
                                        description = Some(TypedFile::new(path))
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Cannot get dir entry type for {:?}, error: {}", f.path(), e)
                            }
                        },
                        Err(e) => warn!(
                            "Cannot list items in directory {:?}, error {}",
                            full_path, e
                        ),
                    }
                }
                // if we have just one chapterized audiobook, let's include it into current directory
                if !self.config.no_dir_collaps
                    && files.is_empty()
                    && subfolders.len() == 1
                    && subfolders[0].is_file
                {
                    let full_path = base_dir.as_ref().join(subfolders.pop().unwrap().path);
                    match self.get_dir_type(&full_path)? {
                        DirType::File {
                            chapters,
                            audio_meta,
                        } => {
                            let f = self
                                .list_dir_file(base_dir, full_path, audio_meta, chapters, true)?;
                            files = f.files;
                            tags = f.tags;
                            is_file = true;
                        }
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::Other,
                                format!("Expecting chapterized file on {:?}", full_path),
                            ))
                        }
                    }
                } else {
                    files.sort_unstable_by(|a, b| a.collate(b));
                    tags = extract_folder_tags(&mut files);
                    subfolders.sort_unstable_by(|a, b| a.compare_as(ordering, b));
                }

                extend_audiofolder(
                    &full_path,
                    AudioFolder {
                        is_file,
                        modified: None,
                        total_time: None,
                        files,
                        subfolders,
                        cover,
                        description,
                        position: None,
                        tags,
                    },
                )
            }
            Err(e) => {
                error!(
                    "Requesting wrong directory {:?} : {}",
                    (&full_path).as_os_str(),
                    e
                );
                Err(e)
            }
        }
    }

    #[allow(clippy::unnecessary_wraps)] // actually as its used in match with function returning results it's better to have Result return type
    fn list_dir_file<P: AsRef<Path>>(
        &self,
        base_dir: P,
        full_path: PathBuf,
        audio_meta: AudioMeta,
        chapters: Vec<Chapter>,
        collapse: bool,
    ) -> Result<AudioFolder, io::Error> {
        let path = full_path.strip_prefix(&base_dir).unwrap();
        let mime = guess_mime_type(&path);
        let mut tags = None;
        if self.config.tags.is_some() {
            let meta = get_audio_properties(&full_path)
                .map_err(|e| warn!("Error extracting meta from {:?}: {}", full_path, e))
                .ok()
                .and_then(|m| m.get_audio_info(&self.config.tags));
            tags = meta.and_then(|m| m.tags);
        }
        let files = chapters
            .into_iter()
            .map(|chap| {
                let new_meta = {
                    AudioMeta {
                        bitrate: audio_meta.bitrate,
                        duration: ((chap.end - chap.start) / 1000) as u32,
                        tags: None, // TODO: consider extracting metadata from chapters too - but what will make sense?
                    }
                };
                Ok(AudioFile {
                    meta: Some(new_meta),
                    path: path_for_chapter(path, &chap, collapse)?,
                    name: format!("{:03} - {}", chap.number, chap.title).into(),
                    section: Some(FileSection {
                        start: chap.start,
                        duration: Some(chap.end - chap.start),
                    }),
                    mime: mime.to_string(),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;

        extend_audiofolder(
            &full_path,
            AudioFolder {
                is_file: true,
                modified: None,
                total_time: None,
                files,
                subfolders: vec![],
                cover: None,
                description: None,
                position: None,
                tags,
            },
        )
    }
}

fn extract_folder_tags(files: &mut Vec<AudioFile>) -> Option<HashMap<String, String>> {
    let mut iter = (files).iter();
    let mut folder_tags = iter
        .next()?
        .meta
        .as_ref()?
        .tags
        .as_ref()?
        .clone()
        .into_iter()
        .map(|(k, v)| (k, Some(v)))
        .collect::<HashMap<_, _>>();

    // folder_tags should contain tags, which are present in all files, where tag is present
    for t in iter {
        if let Some(file_tags) = t.meta.as_ref().and_then(|m| m.tags.as_ref()) {
            for (k, v) in file_tags {
                folder_tags
                    .entry(k.into())
                    .and_modify(|folder_val| {
                        if Some(v) != folder_val.as_ref() {
                            *folder_val = None
                        }
                    })
                    .or_insert_with(|| Some(v.into()));
            }
        }
    }

    // Clear folder_tags of None values
    let folder_tags: HashMap<String, String> = folder_tags
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v)))
        .collect();

    files
        .iter_mut()
        .filter_map(|f| f.meta.as_mut().map(|m| &mut m.tags))
        .for_each(|tags_opt| {
            if let Some(tags) = tags_opt {
                for k in folder_tags.keys() {
                    tags.remove(k);
                }
                if tags.is_empty() {
                    *tags_opt = None;
                }
            }
        });

    if folder_tags.is_empty() {
        None
    } else {
        Some(folder_tags)
    }
}

fn extend_audiofolder<P: AsRef<Path>>(
    full_path: P,
    mut af: AudioFolder,
) -> Result<AudioFolder, io::Error> {
    let last_modification = get_modified(full_path);
    let total_time: u32 = af
        .files
        .iter()
        .map(|f| f.meta.as_ref().map(|m| m.duration).unwrap_or(0))
        .sum();
    af.modified = last_modification.map(TimeStamp::from);
    af.total_time = Some(total_time);
    Ok(af)
}

fn ms_from_time(t: &str) -> Option<u64> {
    let data = t.split(':');
    let res = data
        .map(str::parse::<f32>)
        .try_rfold((0f32, 1f32), |acc, x| {
            x.map(|y| (acc.0 + acc.1 * y, acc.1 * 60f32))
        })
        .map(|r| (r.0 * 1000f32).round() as u64)
        .map_err(|e| error!("Invalid time specification: {} - {}", t, e));
    res.ok()
}

fn chapters_file_path(path: &Path) -> Option<PathBuf> {
    path.file_name()
        .map(|n| {
            let mut f = n.to_owned();
            f.push(".chapters");
            f
        })
        .map(|f| path.with_file_name(f))
}

fn chapters_from_csv(path: &Path) -> Result<Option<Vec<Chapter>>, io::Error> {
    if let Some(chapters_file) = chapters_file_path(path) {
        if chapters_file.is_file() {
            let mut reader = csv::Reader::from_path(&chapters_file)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            let records = reader.records()
                .filter_map(|r| {
                    match r {
                        Err(e) => {
                            error!("Invalid line in chapters file {:?} -  {}", &chapters_file, e);
                            None
                        }
                        Ok(r) => Some(r)
                    }
                })
                .filter_map(|r| {
                    match (r.get(0).map(borrow::ToOwned::to_owned), r.get(1).and_then(ms_from_time),
                        r.get(2).and_then(ms_from_time)) {
                        (Some(title), Some(start), Some(end)) => {
                            Some((title,start, end))
                        }
                        _ => {
                            error!("Invalid line {:?} in chapters file {:?} - missing or invalid fields", r.position(), &chapters_file);
                            None
                        }
                    }
                })
                .enumerate()
                .map(|(number, (title, start, end))| Chapter{number: number as u32,title, start,end})
                .collect();
            return Ok(Some(records));
        }
    }

    Ok(None)
}

fn path_for_chapter(p: &Path, chap: &Chapter, collapse: bool) -> io::Result<PathBuf> {
    let ext = p
        .extension()
        .and_then(OsStr::to_str)
        .map(|e| ".".to_owned() + e)
        .unwrap_or_else(|| "".to_owned());

    let pseudo_file = format!(
        "{:03} - {}$${}-{}$${}",
        chap.number, chap.title, chap.start, chap.end, ext
    );
    let (base, file_name) = if collapse {
        let base = p.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Cannot create path for chapter (no parent) in {:?}", p),
            )
        })?;
        let mut f = p
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "Cannot create path for chapter (invalid file name) in {:?}",
                        p
                    ),
                )
            })?
            .to_string();
        f.push_str("$$");
        f.push_str(&pseudo_file);
        (base, f)
    } else {
        (p, pseudo_file)
    };

    if file_name.len() > 255 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Chapter file name too long",
        ));
    }

    Ok(base.join(file_name))
}

lazy_static! {
    static ref CHAPTER_PSEUDO_RE: Regex = Regex::new(r"\$\$(\d+)-(\d+)\$\$").unwrap();
}

pub fn parse_chapter_path(p: &Path) -> (Cow<Path>, Option<TimeSpan>) {
    let fname = p.file_name().and_then(OsStr::to_str);
    if let Some(fname) = fname {
        if let Some(cap) = CHAPTER_PSEUDO_RE.captures(fname) {
            let start_index = cap.get(0).unwrap().start();
            let start: u64 = cap.get(1).unwrap().as_str().parse().unwrap();
            let end: Option<u64> = cap.get(2).and_then(|g| g.as_str().parse().ok());
            let duration = end.map(|end| end - start);
            let parent = p.parent().unwrap_or_else(|| Path::new(""));
            let path = match fname.find("$$") {
                Some(pos) if pos < start_index => Cow::Owned(parent.join(&fname[..pos])),
                _ => Cow::Borrowed(parent),
            };

            return (path, Some(TimeSpan { start, duration }));
        }
    };

    (Cow::Borrowed(p), None)
}

pub fn list_dir_files_only<P: AsRef<Path>, P2: AsRef<Path>>(
    base_dir: P,
    dir_path: P2,
    allow_symlinks: bool,
) -> Result<Vec<(PathBuf, u64)>, io::Error> {
    let full_path = base_dir.as_ref().join(&dir_path);
    match fs::read_dir(&full_path) {
        Ok(dir_iter) => {
            let mut files = vec![];
            let mut cover = None;
            let mut description = None;
            let allow_symlinks = allow_symlinks;

            fn get_size(p: PathBuf) -> Result<(PathBuf, u64), io::Error> {
                let meta = get_meta(&p)?;
                Ok((p, meta.len()))
            }

            for item in dir_iter {
                match item {
                    Ok(f) => match get_real_file_type(&f, &full_path, allow_symlinks) {
                        Ok(ft) => {
                            let path = f.path();
                            if ft.is_file() {
                                if is_audio(&path) {
                                    files.push(get_size(path)?)
                                } else if cover.is_none() && is_cover(&path) {
                                    cover = Some(get_size(path)?)
                                } else if description.is_none() && is_description(&path) {
                                    description = Some(get_size(path)?)
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Cannot get dir entry type for {:?}, error: {}", f.path(), e)
                        }
                    },
                    Err(e) => warn!(
                        "Cannot list items in directory {:?}, error {}",
                        dir_path.as_ref().as_os_str(),
                        e
                    ),
                }
            }

            if let Some(cover) = cover {
                files.push(cover);
            };

            if let Some(description) = description {
                files.push(description);
            }

            Ok(files)
        }
        Err(e) => {
            error!(
                "Requesting wrong directory {:?} : {}",
                (&full_path).as_os_str(),
                e
            );
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    const TEST_DATA_BASE: &str = "../..";

    #[test]
    fn test_list_dir() {
        media_info::init();
        let lister = FolderLister::new();
        let res = lister.list_dir("/non-existent", "folder", FoldersOrdering::Alphabetical);
        assert!(res.is_err());
        let res = lister.list_dir(TEST_DATA_BASE, "test_data/", FoldersOrdering::Alphabetical);
        assert!(res.is_ok());
        let folder = res.unwrap();
        let num_media_files = 2;
        let num_folders = 2;
        assert_eq!(folder.files.len(), num_media_files);
        assert!(folder.cover.is_some());
        assert!(folder.description.is_some());
        assert_eq!(num_folders, folder.subfolders.len());
    }

    #[test]
    fn test_list_dir_files_only() {
        let res = list_dir_files_only("/non-existent", "folder", false);
        assert!(res.is_err());
        let res = list_dir_files_only(TEST_DATA_BASE, "test_data/", false);
        assert!(res.is_ok());
        let folder = res.unwrap();
        assert_eq!(folder.len(), 5);
    }

    #[test]
    fn test_json() {
        let lister = FolderLister::new();
        let folder = lister
            .list_dir(TEST_DATA_BASE, "test_data/", FoldersOrdering::Alphabetical)
            .unwrap();
        let json = serde_json::to_string(&folder).unwrap();
        println!("JSON: {}", &json);
    }

    #[test]
    fn test_meta() {
        media_info::init();
        let path = Path::new(TEST_DATA_BASE).join(Path::new("test_data/01-file.mp3"));
        let res = get_audio_properties(&path);
        assert!(res.is_ok());
        let media_info = res.unwrap();
        let req_tags = &["title", "album", "artist", "composer"];
        let mut tags = HashSet::new();
        tags.extend(req_tags.into_iter().map(|s| s.to_string()));
        let tags = Some(tags);
        let meta = media_info.get_audio_info(&tags).unwrap();
        assert_eq!(meta.bitrate, 220);
        assert_eq!(meta.duration, 2);
        assert!(meta.tags.is_some());
        let tags = meta.tags.unwrap();

        assert_eq!("KISS", tags.get("title").unwrap());
        assert_eq!("Audioserve", tags.get("album").unwrap());
        assert_eq!("Ivan", tags.get("artist").unwrap());
        assert!(tags.get("composer").is_none());
    }

    #[test]
    fn test_chapters_file() {
        //env_logger::init();
        let path = Path::new(TEST_DATA_BASE).join(Path::new("test_data/01-file.mp3"));
        let chapters = chapters_from_csv(&path).unwrap().unwrap();
        assert_eq!(3, chapters.len());
        let ch3 = &chapters[2];
        assert_eq!("Chapter 3", ch3.title);
        assert_eq!(3000, ch3.end);
    }

    #[test]
    fn test_time_parsing() {
        assert_eq!(Some(1100), ms_from_time("1.1"));
        assert_eq!(
            Some((1000f32 * (2f32 * 3600f32 + 35f32 * 60f32 + 1.1)) as u64),
            ms_from_time("02:35:01.1")
        );
    }

    #[test]
    fn test_create_pseudofile_name() {
        let chap = Chapter {
            number: 1,
            title: "Chapter1".into(),
            start: 1000,
            end: 2000,
        };

        let p = PathBuf::from("stoker/dracula/dracula.m4b");
        let pseudo = path_for_chapter(&p, &chap, false).unwrap();
        assert_eq!(
            pseudo.to_str().unwrap(),
            "stoker/dracula/dracula.m4b/001 - Chapter1$$1000-2000$$.m4b"
        );
        let pseudo = path_for_chapter(&p, &chap, true).unwrap();
        assert_eq!(
            pseudo.to_str().unwrap(),
            "stoker/dracula/dracula.m4b$$001 - Chapter1$$1000-2000$$.m4b"
        );
    }

    #[test]
    fn test_pseudo_file() {
        let fname = format!(
            "kniha/{:3} - {}$${}-{}$${}",
            1, "Usak Jede", 1234, 5678, ".opus"
        );
        let (p, span) = parse_chapter_path(Path::new(&fname));
        let span = span.unwrap();
        assert_eq!(Path::new("kniha"), p);
        assert_eq!(span.start, 1234);
        assert_eq!(span.duration, Some(5678u64 - 1234));
    }

    #[test]
    fn test_pseudo_file2() {
        let f = "stoker/dracula/dracula.m4b/001 - Chapter1$$1000-2000$$.m4b";
        let (p, span) = parse_chapter_path(Path::new(f));
        let span = span.unwrap();
        assert_eq!(p.to_str().unwrap(), "stoker/dracula/dracula.m4b");
        assert_eq!(span.start, 1000);
        assert_eq!(span.duration.unwrap(), 1000);

        let f = "stoker/dracula/dracula.m4b$$001 - Chapter1$$1000-2000$$.m4b";
        let (p, span) = parse_chapter_path(Path::new(f));
        let span = span.unwrap();
        assert_eq!(p.to_str().unwrap(), "stoker/dracula/dracula.m4b");
        assert_eq!(span.start, 1000);
        assert_eq!(span.duration.unwrap(), 1000);
    }
}
