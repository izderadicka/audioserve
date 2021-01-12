use std::borrow;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::audio_meta::{get_audio_properties, Chapter, MediaInfo};
use super::transcode::TimeSpan;
use super::types::*;
use crate::config::get_config;
use crate::util::{get_real_file_type, guess_mime_type, os_to_string};
use regex::Regex;

pub fn list_dir<P: AsRef<Path>, P2: AsRef<Path>>(
    base_dir: P,
    dir_path: P2,
    ordering: FoldersOrdering,
) -> Result<AudioFolder, io::Error> {
    let full_path = base_dir.as_ref().join(&dir_path);
    match get_dir_type(&full_path)? {
        DirType::Dir => list_dir_dir(base_dir, full_path, ordering),
        DirType::File {
            chapters,
            audio_meta,
        } => list_dir_file(base_dir, full_path, audio_meta, chapters),
        DirType::Other => Err(io::Error::new(
            io::ErrorKind::Other,
            "Not folder or chapterised audio file",
        )),
    }
}

enum DirType {
    File {
        chapters: Vec<Chapter>,
        audio_meta: AudioMeta,
    },
    Dir,
    Other,
}

fn split_chapters(dur: u32) -> Vec<Chapter> {
    let chap_length = u64::from(get_config().chapters.duration) * 60 * 1000;
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

fn get_dir_type<P: AsRef<Path>>(path: P) -> Result<DirType, io::Error> {
    let path = path.as_ref();
    let meta = if cfg!(feature = "symlinks") {
        path.metadata()?
    } else {
        path.symlink_metadata()?
    };
    if meta.is_dir() {
        Ok(DirType::Dir)
    } else if meta.is_file() && is_audio(path) {
        let meta =
            get_audio_properties(&path).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        match (meta.get_chapters(), meta.get_audio_info()) {
            (Some(chapters), Some(audio_meta)) => Ok(DirType::File {
                chapters,
                audio_meta,
            }),
            (None, Some(audio_meta)) => match chapters_from_csv(&path)? {
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
                    if is_long_file(Some(&audio_meta)) {
                        let chapters = split_chapters(audio_meta.duration);
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

fn path_for_chapter(p: &Path, chap: &Chapter) -> PathBuf {
    let ext = p
        .extension()
        .and_then(OsStr::to_str)
        .map(|e| ".".to_owned() + e)
        .unwrap_or_else(|| "".to_owned());
    let pseudo_file = format!(
        "{:03} - {}$${}-{}$${}",
        chap.number, chap.title, chap.start, chap.end, ext
    );
    p.join(pseudo_file)
}

lazy_static! {
    static ref CHAPTER_PSEUDO_RE: Regex = Regex::new(r"\$\$(\d+)-(\d*)\$\$").unwrap();
}

pub fn parse_chapter_path(p: &Path) -> (&Path, Option<TimeSpan>) {
    let fname = p.file_name().and_then(OsStr::to_str);
    if let Some(fname) = fname {
        if let Some(cap) = CHAPTER_PSEUDO_RE.captures(fname) {
            let start: u64 = cap.get(1).unwrap().as_str().parse().unwrap();
            let end: Option<u64> = cap.get(2).and_then(|g| g.as_str().parse().ok());
            let duration = end.map(|end| end - start);
            let parent = p.parent().unwrap_or_else(|| Path::new(""));
            return (parent, Some(TimeSpan { start, duration }));
        }
    };

    (p, None)
}

#[allow(clippy::unnecessary_wraps)] // actually as its used in match with function returning results it's better to have Result return type
fn list_dir_file<P: AsRef<Path>>(
    base_dir: P,
    full_path: PathBuf,
    audio_meta: AudioMeta,
    chapters: Vec<Chapter>,
) -> Result<AudioFolder, io::Error> {
    let path = full_path.strip_prefix(&base_dir).unwrap();
    let mime = guess_mime_type(&path);
    let files = chapters
        .into_iter()
        .map(|chap| {
            let new_meta = {
                AudioMeta {
                    bitrate: audio_meta.bitrate,
                    duration: ((chap.end - chap.start) / 1000) as u32,
                }
            };
            AudioFile {
                meta: Some(new_meta),
                path: path_for_chapter(path, &chap),
                name: format!("{:03} - {}", chap.number, chap.title).into(),
                section: Some(FileSection {
                    start: chap.start,
                    duration: Some(chap.end - chap.start),
                }),
                mime: mime.to_string(),
            }
        })
        .collect();

    Ok(AudioFolder {
        files,
        subfolders: vec![],
        cover: None,
        description: None,
    })
}

fn is_long_file(meta: Option<&AudioMeta>) -> bool {
    meta.map(|m| {
        let max_dur = get_config().chapters.from_duration * 60;
        max_dur > 60 * 10 && m.duration > max_dur
    })
    .unwrap_or(false)
}

fn list_dir_dir<P: AsRef<Path>>(
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
            let allow_symlinks = get_config().allow_symlinks;

            for item in dir_iter {
                match item {
                    Ok(f) => match get_real_file_type(&f, &full_path, allow_symlinks) {
                        Ok(ft) => {
                            let path = f.path().strip_prefix(&base_dir).unwrap().into();
                            if ft.is_dir() {
                                subfolders.push(AudioFolderShort::from_dir_entry(
                                    &f, path, ordering, false,
                                )?)
                            } else if ft.is_file() {
                                if is_audio(&path) {
                                    let mime = guess_mime_type(&path);
                                    let audio_file_path = base_dir.as_ref().join(&path);
                                    let meta = match get_audio_properties(&audio_file_path) {
                                        Ok(meta) => meta,
                                        Err(e) => {
                                            error!("Cannot add file because error in extraction audio meta: {}",e);
                                            continue;
                                        }
                                    };

                                    if let Some(_chapters) = meta.get_chapters() {
                                        // we do have chapters so let present this file as folder
                                        subfolders.push(AudioFolderShort::from_dir_entry(
                                            &f, path, ordering, true,
                                        )?)
                                    } else {
                                        let meta = meta.get_audio_info();
                                        if is_long_file((&meta).as_ref())
                                            || chapters_file_path(&audio_file_path)
                                                .and_then(
                                                    |p| if p.is_file() { Some(()) } else { None },
                                                )
                                                .is_some()
                                        {
                                            // file is bigger then limit present as folder
                                            subfolders.push(AudioFolderShort::from_dir_entry(
                                                &f, path, ordering, true,
                                            )?)
                                        } else {
                                            files.push(AudioFile {
                                                meta,
                                                path,
                                                name: os_to_string(f.file_name()).into(),
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
            files.sort_unstable_by(|a, b| a.name.cmp(&b.name));
            subfolders.sort_unstable_by(|a, b| a.compare_as(ordering, b));

            Ok(AudioFolder {
                files,
                subfolders,
                cover,
                description,
            })
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

#[cfg(feature = "folder-download")]
pub fn list_dir_files_only<P: AsRef<Path>, P2: AsRef<Path>>(
    base_dir: P,
    dir_path: P2,
) -> Result<Vec<(PathBuf, u64)>, io::Error> {
    let full_path = base_dir.as_ref().join(&dir_path);
    match fs::read_dir(&full_path) {
        Ok(dir_iter) => {
            let mut files = vec![];
            let mut cover = None;
            let mut description = None;
            let allow_symlinks = get_config().allow_symlinks;

            fn get_size(p: PathBuf) -> Result<(PathBuf, u64), io::Error> {
                let meta = p.metadata()?;
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
    use crate::config::init::init_default_config;
    use serde_json;

    #[test]
    fn test_list_dir() {
        init_default_config();
        media_info::init();
        let res = list_dir("/non-existent", "folder", FoldersOrdering::Alphabetical);
        assert!(res.is_err());
        let res = list_dir("./", "test_data/", FoldersOrdering::Alphabetical);
        assert!(res.is_ok());
        let folder = res.unwrap();
        let num_media_files = 2;
        let num_folders = 2;
        assert_eq!(folder.files.len(), num_media_files);
        assert!(folder.cover.is_some());
        assert!(folder.description.is_some());
        assert_eq!(num_folders, folder.subfolders.len());
    }

    #[cfg(feature = "folder-download")]
    #[test]
    fn test_list_dir_files_only() {
        init_default_config();
        let res = list_dir_files_only("/non-existent", "folder");
        assert!(res.is_err());
        let res = list_dir_files_only("./", "test_data/");
        assert!(res.is_ok());
        let folder = res.unwrap();
        assert_eq!(folder.len(), 5);
    }

    #[test]
    fn test_json() {
        init_default_config();
        let folder = list_dir("./", "test_data/", FoldersOrdering::Alphabetical).unwrap();
        let json = serde_json::to_string(&folder).unwrap();
        println!("JSON: {}", &json);
    }

    #[test]
    fn test_meta() {
        media_info::init();
        let path = Path::new("./test_data/01-file.mp3");
        let res = get_audio_properties(path);
        assert!(res.is_ok());
        let media_info = res.unwrap();
        let meta = media_info.get_audio_info().unwrap();
        assert_eq!(meta.bitrate, 220);
        assert_eq!(meta.duration, 2);
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
    fn test_chapters_file() {
        //env_logger::init();
        let path = Path::new("./test_data/01-file.mp3");
        let chapters = chapters_from_csv(path).unwrap().unwrap();
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
}
