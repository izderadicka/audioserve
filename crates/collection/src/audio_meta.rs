use mime_guess::Mime;
use serde_derive::Serialize;
use crate::error::{Error, Result};
use std::{cmp::Ordering, path::{Path, PathBuf}, time::SystemTime};
use crate::util::guess_mime_type;
use unicase::UniCase;

#[derive(Debug, Serialize)]
pub struct TypedFile {
    pub path: PathBuf,
    pub mime: String,
}

impl TypedFile {
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        let path = path.into();
        let mime = guess_mime_type(&path);
        TypedFile {
            path,
            mime: mime.as_ref().into(),
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileSection {
    pub start: u64,
    pub duration: Option<u64>,
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AudioFile {
    #[serde(with = "unicase_serde::unicase")]
    pub name: UniCase<String>,
    pub path: PathBuf,
    pub meta: Option<AudioMeta>,
    pub mime: String,
    pub section: Option<FileSection>,
}

#[derive(Debug, Serialize)]
pub struct AudioFolder {
    pub files: Vec<AudioFile>,
    pub subfolders: Vec<AudioFolderShort>,
    pub cover: Option<TypedFile>, // cover is file in folder - either jpg or png
    pub description: Option<TypedFile>, // description is file in folder - either txt, html, md
}

#[derive(Clone, Copy)]
pub enum FoldersOrdering {
    Alphabetical,
    RecentFirst,
}

impl FoldersOrdering {
    pub fn from_letter(l: &str) -> Self {
        match l {
            "m" => FoldersOrdering::RecentFirst,
            _ => FoldersOrdering::Alphabetical,
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct AudioMeta {
    pub duration: u32, // duration in seconds, if available
    pub bitrate: u32,  // bitrate in kB/s
}

#[derive(Clone, Copy, Debug)]
pub struct TimeSpan {
    pub start: u64,
    pub duration: Option<u64>,
}

impl std::fmt::Display for TimeSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        match self.duration {
            Some(d) => write!(f, "{}-{}", self.start, d),
            None => write!(f, "{}", self.start),
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AudioFolderShort {
    #[serde(with = "unicase_serde::unicase")]
    pub name: UniCase<String>,
    pub path: PathBuf,
    pub is_file: bool,
    #[serde(skip)] // May make it visible in future
    pub modified: Option<SystemTime>,
}

impl AudioFolderShort {
    pub fn from_path<P: AsRef<Path>>(base_path: &Path, p: P) -> Self {
        let p = p.as_ref();
        AudioFolderShort {
            name: p.file_name().unwrap().to_string_lossy().into(),
            path: p.strip_prefix(base_path).unwrap().into(),
            is_file: false,
            modified: None,
        }
    }

    pub fn from_dir_entry(
        f: &std::fs::DirEntry,
        path: PathBuf,
        ordering: FoldersOrdering,
        is_file: bool,
    ) -> std::result::Result<Self, std::io::Error> {
        Ok(AudioFolderShort {
            path,
            name: f.file_name().to_string_lossy().into(),
            is_file,

            modified: {
                if let FoldersOrdering::RecentFirst = ordering {
                    Some(f.metadata()?.modified()?)
                } else {
                    None
                }
            },
        })
    }

    pub fn from_path_and_name(name: String, path: PathBuf, is_file: bool) -> Self {
        AudioFolderShort {
            name: name.into(),
            path,
            is_file,
            modified: None,
        }
    }

    pub fn compare_as(&self, ord: FoldersOrdering, other: &Self) -> Ordering {
        match ord {
            FoldersOrdering::Alphabetical => self.name.cmp(&other.name),
            FoldersOrdering::RecentFirst => match (self.modified, other.modified) {
                (Some(ref a), Some(ref b)) => b.cmp(a),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            },
        }
    }
}

pub struct Chapter {
    pub number: u32,
    pub title: String,
    pub start: u64,
    pub end: u64,
}

fn has_subtype(mime: &Mime, subtypes: &[&str]) -> bool {
    subtypes.iter().any(|&s| s == mime.subtype())
}

const AUDIO: &[&str] = &[
    "ogg",
    "mpeg",
    "aac",
    "m4a",
    "m4b",
    "x-matroska",
    "flac",
    "webm",
];
pub fn is_audio<P: AsRef<Path>>(path: P) -> bool {
    let mime = guess_mime_type(path);
    mime.type_() == "audio" && has_subtype(&mime, AUDIO)
}

const COVERS: &[&str] = &["jpeg", "png"];

pub fn is_cover<P: AsRef<Path>>(path: P) -> bool {
    let mime = guess_mime_type(path);
    mime.type_() == "image" && has_subtype(&mime, COVERS)
}

const DESCRIPTIONS: &[&str] = &["html", "plain", "markdown"];

pub fn is_description<P: AsRef<Path>>(path: P) -> bool {
    let mime = guess_mime_type(path);
    mime.type_() == "text" && has_subtype(&mime, DESCRIPTIONS)
}



/// trait to generalize access to media metadata
/// (so that underlying library can be easily changed)
pub trait MediaInfo<'a>: Sized {
    fn get_audio_info(&self) -> Option<AudioMeta>;
    fn get_chapters(&self) -> Option<Vec<Chapter>>;
    fn has_chapters(&self) -> bool;
}

mod libavformat {
    use super::*;
    use std::sync::Once;

    static INIT_LIBAV: Once = Once::new();

    pub fn init() {
        INIT_LIBAV.call_once(media_info::init)
    }

    pub struct Info {
        media_file: media_info::MediaFile,
    }
    impl<'a> MediaInfo<'a> for Info {
        fn get_audio_info(&self) -> Option<AudioMeta> {
            Some(AudioMeta {
                duration: (self.media_file.duration() as f32 / 1000.0).round() as u32,
                bitrate: self.media_file.bitrate(),
            })
        }

        fn has_chapters(&self) -> bool {
            self.media_file.chapters_count() > 1
        }
        
        fn get_chapters(&self) -> Option<Vec<Chapter>> {
            self.media_file.chapters().map(|l| {
                l.into_iter()
                    .map(|c| Chapter {
                        number: c.num as u32,
                        title: c.title,
                        start: c.start,
                        end: c.end,
                    })
                    .collect()
            })
        }
    }

    impl Info {
        pub fn from_file(path: &Path) -> Result<Info> {
            match path.as_os_str().to_str() {
                Some(fname) => Ok(Info { media_file:  media_info::MediaFile::open(fname)?}),
                None => {
                    error!("Invalid file name {:?}, not utf-8", path);
                    Err(Error::InvalidFileName)
                }
            }
        }
    }
}

pub fn get_audio_properties(audio_file_name: &Path) -> Result<impl MediaInfo> {
    libavformat::Info::from_file(audio_file_name)
}

pub fn init_media_lib() {
    libavformat::init()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_audio() {
        assert!(is_audio("my/song.mp3"));
        assert!(is_audio("other/chapter.opus"));
        assert!(!is_audio("cover.jpg"));
    }

    #[test]
    fn test_is_cover() {
        assert!(is_cover("cover.jpg"));
        assert!(!is_cover("my/song.mp3"));
    }

    #[test]
    fn test_is_description() {
        assert!(!is_description("cover.jpg"));
        assert!(is_description("about.html"));
        assert!(is_description("about.txt"));
        assert!(is_description("some/folder/text.md"));
    }

}
