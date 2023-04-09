use std::borrow::{self, Cow};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::{fs, mem};

use super::audio_meta::*;
use crate::collator::Collate;
use crate::common::CollectionOptions;
use crate::playlist::{is_playlist, Playlist};
use crate::util::{get_meta, get_modified, get_real_file_type, guess_mime_type};
use lazy_static::lazy_static;
use regex::Regex;

#[derive(Debug)]
pub struct FullAudioMeta {
    chapters: Vec<Chapter>,
    audio_meta: AudioMeta,
    has_cover: bool,
    has_description: bool,
}

pub enum DirType {
    File(FullAudioMeta),
    Dir,
    Other,
}

#[derive(Debug, Clone)]
pub(crate) struct FolderOptions {
    pub chapters_duration: u32,
    pub chapters_from_duration: u32,
    pub ignore_chapters_meta: bool,
    pub allow_symlinks: bool,
    pub no_dir_collaps: bool,
    pub natural_files_ordering: bool,
    pub tags: Option<HashSet<String>>,
    pub cd_folder_regex: Option<Regex>,
    #[cfg(feature = "tags-encoding")]
    pub tags_encoding: Option<String>,
}

impl From<CollectionOptions> for FolderOptions {
    fn from(o: CollectionOptions) -> Self {
        Self {
            chapters_duration: o.chapters_duration,
            chapters_from_duration: o.chapters_from_duration,
            ignore_chapters_meta: o.ignore_chapters_meta,
            allow_symlinks: o.allow_symlinks,
            no_dir_collaps: o.no_dir_collaps,
            natural_files_ordering: o.natural_files_ordering,
            tags: o.tags,
            cd_folder_regex: o.cd_folder_regex,
            #[cfg(feature = "tags-encoding")]
            tags_encoding: o.tags_encoding,
        }
    }
}

#[derive(Clone)]
pub(crate) struct FolderLister {
    config: FolderOptions,
}

impl FolderLister {
    pub(crate) fn new_with_options(config: FolderOptions) -> Self {
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
            DirType::Dir => {
                if self.is_collapsable_folder(&full_path) {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!(
                            "Directory {:?} is collapsed, should not be scanned directly",
                            full_path
                        ),
                    ));
                }
                self.list_dir_dir(base_dir, full_path, ordering, true)
            }
            DirType::File(full_meta) => self.list_dir_file(base_dir, full_path, full_meta, false),
            DirType::Other => Err(io::Error::new(
                io::ErrorKind::Other,
                "Not folder or chapterised audio file",
            )),
        }
    }

    pub(crate) fn collapse_cd_enabled(&self) -> bool {
        self.config.cd_folder_regex.is_some()
    }

    pub(crate) fn is_collapsable_folder(&self, p: impl AsRef<Path>) -> bool {
        self.config
            .cd_folder_regex
            .as_ref()
            .and_then(|re| {
                let name = p.as_ref().file_name()?.to_str()?;
                Some(re.is_match(name))
            })
            .unwrap_or(false)
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
            #[cfg(feature = "tags-encoding")]
            let audio_info = get_audio_properties(path, self.config.tags_encoding.as_ref());
            #[cfg(not(feature = "tags-encoding"))]
            let audio_info = get_audio_properties(path);
            let meta = audio_info.map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            let has_cover = meta.has_cover();
            let has_description = meta.has_description();
            match (meta.get_chapters(), meta.get_audio_info(&self.config.tags)) {
                (Some(chapters), Some(audio_meta)) => Ok(DirType::File(FullAudioMeta {
                    chapters,
                    audio_meta,
                    has_cover,
                    has_description,
                })),
                (None, Some(audio_meta)) => match chapters_from_csv(path)? {
                    Some(chapters) => {
                        if chapters.len() > 1 {
                            Ok(DirType::File(FullAudioMeta {
                                chapters,
                                audio_meta,
                                has_cover,
                                has_description,
                            }))
                        } else {
                            error!("Chapter file for {:?} has less then two chapters!", &path);
                            Ok(DirType::Other)
                        }
                    }
                    None => {
                        if self.is_long_file(Some(&audio_meta)) {
                            let chapters = self.split_chapters(audio_meta.duration);
                            Ok(DirType::File(FullAudioMeta {
                                chapters,
                                audio_meta,
                                has_cover,
                                has_description,
                            }))
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
        extract_tags: bool,
    ) -> Result<AudioFolder, io::Error> {
        match fs::read_dir(&full_path) {
            Ok(dir_iter) => {
                let mut files = vec![];
                let mut subfolders = vec![];
                let mut cover = None;
                let mut description = None;
                let mut playlist: Option<Playlist> = None;
                let tags;
                let mut is_file = false;
                let mut is_collapsed = false;
                let allow_symlinks = self.config.allow_symlinks;

                for item in dir_iter {
                    match item {
                        Ok(f) => match get_real_file_type(&f, &full_path, allow_symlinks) {
                            Ok(ft) => {
                                let long_path = f.path();
                                let path = long_path.strip_prefix(&base_dir).unwrap().into();
                                if ft.is_dir() {
                                    subfolders
                                        .push(AudioFolderShort::from_dir_entry(&f, path, false)?)
                                } else if ft.is_file() {
                                    if is_audio(&path) {
                                        let mime = guess_mime_type(&path);
                                        #[cfg(feature = "tags-encoding")]
                                        let audio_info = get_audio_properties(
                                            &long_path,
                                            self.config.tags_encoding.as_ref(),
                                        );
                                        #[cfg(not(feature = "tags-encoding"))]
                                        let audio_info = get_audio_properties(&long_path);
                                        let meta = match audio_info {
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
                                            if self.is_long_file(meta.as_ref())
                                                || chapters_file_path(&long_path)
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
                                    } else if playlist.is_none() && is_playlist(&path) {
                                        playlist = Playlist::new(&long_path, &full_path)
                                            .map_err(|e| {
                                                error!(
                                                    "Error reading playlist {:?}: {}",
                                                    long_path, e
                                                )
                                            })
                                            .ok();
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
                        DirType::File(full_meta) => {
                            let f = self.list_dir_file(base_dir, full_path, full_meta, true)?;
                            files = f.files;
                            tags = f.tags;
                            is_file = true;
                            if cover.is_none() {
                                cover = f.cover;
                            }
                            if description.is_none() {
                                description = f.description;
                            }
                            // TODO: Should this be also collapsed?
                            //is_collapsed = true;
                        }
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::Other,
                                format!("Expecting chapterized file on {:?}", full_path),
                            ))
                        }
                    }
                } else {
                    let mut sorted = false;

                    let file_sorter = if self.config.natural_files_ordering {
                        |a: &AudioFile, b: &AudioFile| a.collate_natural(b)
                    } else {
                        |a: &AudioFile, b: &AudioFile| a.collate(b)
                    };

                    if let Some(playlist) = playlist {
                        debug!("We have playlist {:?}", playlist.path());
                        // remove covered subfolders
                        if playlist.has_subfolders() {
                            subfolders = mem::take(&mut subfolders)
                                .into_iter()
                                .filter(|d| !playlist.is_covering(&d.path))
                                .collect();
                        }
                        // Use items from playlist
                        let path_in_folder = full_path.strip_prefix(base_dir).unwrap();
                        let mut old_files: HashMap<_, _> = mem::take(&mut files)
                            .into_iter()
                            .map(|f| (f.path.strip_prefix(path_in_folder).unwrap().to_owned(), f))
                            .collect();
                        let items = playlist.to_items();
                        for item_path in items {
                            if let Some(existing) = old_files.remove(&item_path) {
                                files.push(existing)
                            } else {
                                warn!("Not implemented yet for subdir PL item {:?}", item_path);
                            }

                            sorted = true;
                        }
                    } else {
                        if !subfolders.is_empty() {
                            if let Some(ref re) = self.config.cd_folder_regex {
                                let can_collapse =
                                    |f: &AudioFolderShort| !f.is_file && re.is_match(&f.name);
                                let will_collapse = subfolders.iter().any(can_collapse);
                                if will_collapse {
                                    is_collapsed = true;
                                    sorted = true;
                                    debug!("Can collapse CD subfolders on path {:?}", full_path);
                                    let mut folders = mem::take(&mut subfolders);
                                    files.sort_unstable_by(file_sorter);
                                    folders.sort_unstable_by(|a, b| {
                                        if self.config.natural_files_ordering {
                                            a.collate_natural(b)
                                        } else {
                                            a.collate(b)
                                        }
                                    });
                                    for fld in folders {
                                        if can_collapse(&fld) {
                                            let prefix: String = fld.name.into();
                                            let subdir_path = base_dir.as_ref().join(&fld.path);
                                            let subdir_name = fld.path.file_name();
                                            let mut subdir = self.list_dir_dir(
                                                base_dir.as_ref(),
                                                subdir_path,
                                                FoldersOrdering::Alphabetical,
                                                false,
                                            )?;
                                            if !subdir.subfolders.is_empty() {
                                                warn!("CD folder contains subfolders, these will not be visible");
                                            }
                                            subdir.files.sort_unstable_by(file_sorter);
                                            for mut f in subdir.files {
                                                if let (Some(file_name), Some(subdir_name)) =
                                                    (f.path.file_name(), subdir_name)
                                                {
                                                    f.name =
                                                        (prefix.clone() + " " + &f.name).into();
                                                    let mut new_file_name = subdir_name.to_owned();
                                                    new_file_name.push("$$");
                                                    new_file_name.push(file_name);
                                                    let mut new_path = fld.path.clone();
                                                    new_path.set_file_name(new_file_name);
                                                    f.path = new_path;
                                                    files.push(f);
                                                } else {
                                                    warn!(
                                                        "CD subfolder in wrong position: ${:?}",
                                                        fld.path
                                                    );
                                                }
                                            }
                                        } else {
                                            subfolders.push(fld);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !sorted {
                        files.sort_unstable_by(file_sorter);
                    }
                    tags = if extract_tags {
                        extract_folder_tags(&mut files)
                    } else {
                        None
                    };
                    subfolders.sort_unstable_by(|a, b| a.compare_as(ordering, b));
                }

                extend_audiofolder(
                    &full_path,
                    AudioFolder {
                        is_file,
                        is_collapsed,
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
                error!("Requesting wrong directory {:?} : {}", full_path, e);
                Err(e)
            }
        }
    }

    #[allow(clippy::unnecessary_wraps)] // actually as its used in match with function returning results it's better to have Result return type
    fn list_dir_file<P: AsRef<Path>>(
        &self,
        base_dir: P,
        full_path: PathBuf,
        full_meta: FullAudioMeta,
        collapse: bool,
    ) -> Result<AudioFolder, io::Error> {
        let path = full_path.strip_prefix(&base_dir).unwrap();
        let mime = guess_mime_type(path);
        let mut tags = None;
        if self.config.tags.is_some() {
            #[cfg(feature = "tags-encoding")]
            let audio_info = get_audio_properties(&full_path, self.config.tags_encoding.as_ref());
            #[cfg(not(feature = "tags-encoding"))]
            let audio_info = get_audio_properties(&full_path);
            let meta = audio_info
                .map_err(|e| warn!("Error extracting meta from {:?}: {}", full_path, e))
                .ok()
                .and_then(|m| m.get_audio_info(&self.config.tags));
            tags = meta.and_then(|m| m.tags);
        }
        let files = full_meta
            .chapters
            .into_iter()
            .map(|chap| {
                let new_meta = {
                    AudioMeta {
                        bitrate: full_meta.audio_meta.bitrate,
                        duration: ((chap.end - chap.start) / 1000) as u32,
                        tags: None, // TODO: consider extracting metadata from chapters too - but what will make sense?
                    }
                };
                let (name, path) = name_and_path_for_chapter(path, &chap, collapse)?;
                Ok(AudioFile {
                    meta: Some(new_meta),
                    path,
                    name: name.into(),
                    section: Some(FileSection {
                        start: chap.start,
                        duration: Some(chap.end - chap.start),
                    }),
                    mime: mime.to_string(),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;

        let self_file = |include| {
            if include {
                Some(TypedFile {
                    path: path.into(),
                    mime: mime.to_string(),
                })
            } else {
                None
            }
        };
        extend_audiofolder(
            &full_path,
            AudioFolder {
                is_file: true,
                is_collapsed: false,
                modified: None,
                total_time: None,
                files,
                subfolders: vec![],
                cover: self_file(full_meta.has_cover),
                description: self_file(full_meta.has_description),
                position: None,
                tags,
            },
        )
    }
}

fn extract_folder_tags(files: &mut [AudioFile]) -> Option<HashMap<String, String>> {
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

const MAX_CHAPTER_SIZE: usize = 255;
fn name_and_path_for_chapter(
    p: &Path,
    chap: &Chapter,
    collapse: bool,
) -> io::Result<(String, PathBuf)> {
    let ext = p
        .extension()
        .and_then(OsStr::to_str)
        .map(|e| ".".to_owned() + e)
        .unwrap_or_else(|| "".to_owned());

    // Must sanitize name, should not contain /
    let mut pseudo_name = chap.title.replace('/', "-");
    pseudo_name = format!("{:03} - {pseudo_name}", chap.number);
    let name_suffix = format!("$${}-{}$${}", chap.start, chap.end, ext);
    let allowed_len = MAX_CHAPTER_SIZE - name_suffix.len();

    let name_sz = pseudo_name.len();
    if name_sz > allowed_len {
        let sz = (allowed_len - 3) / 2;
        let mut end = 0;
        let mut start = 0;
        let mut pos = 0;
        for (idx, _) in pseudo_name.char_indices() {
            if start == 0 && idx > sz {
                start = pos
            }
            pos = idx;
            if name_sz - pos <= sz {
                end = pos;
                break;
            }
        }
        // Now it should be ok to slice string

        pseudo_name = pseudo_name[0..start].to_string() + "..." + &pseudo_name[end..];
        debug_assert!(pseudo_name.len() <= allowed_len);
    }
    let pseudo_file = pseudo_name.clone() + &name_suffix;
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

    Ok((pseudo_name, base.join(file_name)))
}

lazy_static! {
    static ref CHAPTER_SPAN_RE: Regex = Regex::new(r"(\d+)-(\d+)").unwrap();
}

fn parse_span(s: &str) -> Option<TimeSpan> {
    if let Some(cap) = CHAPTER_SPAN_RE.captures(s) {
        // can unwrap because of regex
        let start: u64 = cap.get(1).unwrap().as_str().parse().unwrap();
        let end: Option<u64> = cap.get(2).and_then(|g| g.as_str().parse().ok());
        let duration = end.map(|end| end - start);

        Some(TimeSpan { start, duration })
    } else {
        None
    }
}

pub fn parse_chapter_path(p: &Path) -> (Cow<Path>, Option<TimeSpan>) {
    let fname = p.file_name().and_then(OsStr::to_str);
    if let Some(fname) = fname {
        let parts: Vec<_> = fname.split("$$").collect();
        let sz = parts.len();
        match sz {
            1 => (Cow::Borrowed(p), None),
            2 | 3 | 4 => {
                let parent = p.parent().unwrap_or_else(|| Path::new(""));
                if sz == 2 {
                    (Cow::Owned(parent.join(parts[0]).join(parts[1])), None)
                } else {
                    let span = parse_span(parts[sz - 2]);
                    if let Some(span) = span {
                        if sz == 3 {
                            (Cow::Borrowed(parent), Some(span))
                        } else {
                            let p = parent.join(parts[0]);
                            (Cow::Owned(p), Some(span))
                        }
                    } else {
                        warn!(
                            "Invalid file name - this {} should be time chappter time span",
                            parts[sz - 2]
                        );
                        (Cow::Borrowed(p), None)
                    }
                }
            }
            _ => {
                warn!("Unsupported file name - $$  separators : {}", fname);
                (Cow::Borrowed(p), None)
            }
        }
    } else {
        (Cow::Borrowed(p), None)
    }
}

pub fn list_dir_files_only(
    base_dir: impl AsRef<Path>,
    dir_path: impl AsRef<Path>,
    allow_symlinks: bool,
) -> Result<Vec<(PathBuf, String, u64)>, io::Error> {
    list_dir_files_ext(
        base_dir,
        dir_path,
        allow_symlinks,
        None,
        |p| -> Result<String, io::Error> {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid file name"))
        },
    )
}

pub fn list_dir_files_with_subdirs(
    base_dir: impl AsRef<Path>,
    dir_path: impl AsRef<Path>,
    allow_symlinks: bool,
    include_subdirs: Regex,
) -> Result<Vec<(PathBuf, String, u64)>, io::Error> {
    let full_path = base_dir.as_ref().join(&dir_path);
    list_dir_files_ext(
        base_dir,
        dir_path,
        allow_symlinks,
        Some(include_subdirs),
        |p| {
            let subdir = p
                .strip_prefix(&full_path)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Invalid path {}", e)))?
                .parent();

            let name = p.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::Other, "Invalid file name - not UTF8")
            })?;
            if !subdir
                .and_then(|p| p.file_name().map(|n| n.is_empty()))
                .unwrap_or(true)
            {
                let folder = p
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|f| f.to_str())
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::Other, "Invalid folder name - not UTF8")
                    })?;

                Ok(folder.to_string() + " " + name)
            } else {
                Ok(name.into())
            }
        },
    )
}

fn list_dir_files_ext<F>(
    base_dir: impl AsRef<Path>,
    dir_path: impl AsRef<Path>,
    allow_symlinks: bool,
    include_subdirs: Option<Regex>,
    name_fn: F,
) -> Result<Vec<(PathBuf, String, u64)>, io::Error>
where
    F: Fn(&Path) -> Result<String, io::Error>,
{
    let full_path = base_dir.as_ref().join(&dir_path);
    match fs::read_dir(&full_path) {
        Ok(dir_iter) => {
            let mut files = vec![];
            let mut cover = None;
            let mut description = None;
            let allow_symlinks = allow_symlinks;

            let get_size_and_name = |p: PathBuf| -> Result<(PathBuf, String, u64), io::Error> {
                let meta = get_meta(&p)?;
                let name = name_fn(&p)?;
                Ok((p, name, meta.len()))
            };

            for item in dir_iter {
                match item {
                    Ok(f) => match get_real_file_type(&f, &full_path, allow_symlinks) {
                        Ok(ft) => {
                            let path = f.path();
                            if ft.is_file() {
                                if is_audio(&path) {
                                    files.push(get_size_and_name(path)?)
                                } else if cover.is_none() && is_cover(&path) {
                                    cover = Some(get_size_and_name(path)?)
                                } else if description.is_none() && is_description(&path) {
                                    description = Some(get_size_and_name(path)?)
                                }
                            } else if ft.is_dir() {
                                if let Some(ref re) = include_subdirs {
                                    let name = f.file_name();
                                    let name = name.to_str().ok_or_else(|| {
                                        io::Error::new(io::ErrorKind::Other, "Non UTF-8 name")
                                    })?;
                                    if re.is_match(name) {
                                        let subdir = f.path();
                                        let di = fs::read_dir(&subdir)?;
                                        for item in di {
                                            let f = item?;
                                            let ft =
                                                get_real_file_type(&f, &subdir, allow_symlinks)?;
                                            let file_path = f.path();
                                            if ft.is_file() && is_audio(&file_path) {
                                                files.push(get_size_and_name(file_path)?)
                                            }
                                        }
                                    }
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
            error!("Requesting wrong directory {:?} : {}", full_path, e);
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
        let lister = FolderLister::new_with_options(CollectionOptions::default().into());
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
        let lister = FolderLister::new_with_options(CollectionOptions::default().into());
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
        #[cfg(feature = "tags-encoding")]
        let res = get_audio_properties(&path, None as Option<String>);
        #[cfg(not(feature = "tags-encoding"))]
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
        let (_, pseudo) = name_and_path_for_chapter(&p, &chap, false).unwrap();
        assert_eq!(
            pseudo.to_str().unwrap(),
            "stoker/dracula/dracula.m4b/001 - Chapter1$$1000-2000$$.m4b"
        );
        let (_, pseudo) = name_and_path_for_chapter(&p, &chap, true).unwrap();
        assert_eq!(
            pseudo.to_str().unwrap(),
            "stoker/dracula/dracula.m4b$$001 - Chapter1$$1000-2000$$.m4b"
        );
    }

    #[test]
    fn test_long_chapter_name() {
        let long_name = "As I ponder the complexities of the world, I am struck by the fragility of human existence and the interconnectedness of all things. From the tiniest microbe to the vast expanses of the universe, everything is connected in ways we may never fully comprehend.";
        let chap = Chapter {
            number: 1,
            title: long_name.into(),
            start: 1000,
            end: 2000,
        };

        let correct = "stoker/dracula/dracula.m4b/001 - As I ponder the complexities of the world, I am struck by the fragility of human existence and the interconnect... tiniest microbe to the vast expanses of the universe, everything is connected in ways we may never fully comprehend.$$1000-2000$$.m4b";

        let p = PathBuf::from("stoker/dracula/dracula.m4b");
        let (_, pseudo) = name_and_path_for_chapter(&p, &chap, false).unwrap();
        assert_eq!(correct, pseudo.to_str().unwrap());

        let limit_case: String = (0..237).into_iter().map(|_| "X").collect();
        let chap2 = Chapter {
            number: 1,
            title: limit_case,
            start: 1000,
            end: 2000,
        };
        let p2 = PathBuf::from("");
        let (_, name) = name_and_path_for_chapter(&p2, &chap2, false).unwrap();
        let name = name.to_str().unwrap();
        assert_eq!(254, name.len());
        assert!(name.find("...").unwrap() > 100);

        let cesky =
            "příliš žluťoučký kůň úpěl ďábelské ódy. PŘÍLIŠ ŽLUŤOUČKÝ KŮŇ ÚPĚL ĎÁBELSKÉ ÓDY.";
        let cesky = cesky.to_string() + cesky + cesky + cesky + cesky + cesky;
        let chap3 = Chapter {
            number: 1,
            title: cesky,
            start: 1000,
            end: 2000,
        };
        let p2 = PathBuf::from("");
        let (_, name) = name_and_path_for_chapter(&p2, &chap3, false).unwrap();
        let name = name.to_str().unwrap();
        assert_eq!(253, name.len());
        assert!(name.find("...").unwrap() > 100);
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

        let f = "stoker/dracula/dracula.m4b/001 - Chapter1$$1000-2000$$";
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

        let f = "stoker/dracula/dracula.m4b$$001 - Chapter1$$1000-2000$$";
        let (p, span) = parse_chapter_path(Path::new(f));
        let span = span.unwrap();
        assert_eq!(p.to_str().unwrap(), "stoker/dracula/dracula.m4b");
        assert_eq!(span.start, 1000);
        assert_eq!(span.duration.unwrap(), 1000);
    }

    #[test]
    fn test_pseudo_file3() {
        let f = "Follet Ken/Srsen leta v noci/CD1$$01 Srsen leta v noci.opus";
        let (p, span) = parse_chapter_path(Path::new(f));
        assert!(span.is_none());
        assert_eq!(
            p.to_str().unwrap(),
            "Follet Ken/Srsen leta v noci/CD1/01 Srsen leta v noci.opus"
        );
    }
}
