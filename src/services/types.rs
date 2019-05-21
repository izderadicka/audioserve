use super::transcode::{QualityLevel, TranscodingFormat};
use crate::config::get_config;
use crate::util::os_to_string;
use mime::Mime;
use mime_guess::guess_mime_type;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
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

pub struct AudioFormat {
    pub ffmpeg: &'static str,
    pub mime: Mime,
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

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct AudioMeta {
    pub duration: u32, // duration in seconds, if available
    pub bitrate: u32,  // bitrate in kB/s
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
            name: p.file_name().unwrap().to_str().unwrap().into(),
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
    ) -> Result<Self, std::io::Error> {
        Ok(AudioFolderShort {
            path,
            name: os_to_string(f.file_name()).into(),
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

    #[cfg(feature = "search-cache")]
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

#[derive(Debug, Serialize)]
pub struct Collections {
    pub folder_download: bool,
    pub count: u32,
    pub names: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct TranscodingSummary {
    bitrate: u32,
    name: &'static str,
}

impl From<TranscodingFormat> for TranscodingSummary {
    fn from(f: TranscodingFormat) -> Self {
        TranscodingSummary {
            bitrate: f.bitrate(),
            name: f.format_name(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Transcodings {
    pub max_transcodings: u32,
    pub low: TranscodingSummary,
    pub medium: TranscodingSummary,
    pub high: TranscodingSummary,
}

impl Transcodings {
    pub fn new() -> Self {
        let cfg = get_config();
        Transcodings {
            max_transcodings: cfg.transcoding.max_parallel_processes,
            low: cfg.transcoding.get(QualityLevel::Low).into(),
            medium: cfg.transcoding.get(QualityLevel::Medium).into(),
            high: cfg.transcoding.get(QualityLevel::High).into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AudioFolder {
    pub files: Vec<AudioFile>,
    pub subfolders: Vec<AudioFolderShort>,
    pub cover: Option<TypedFile>, // cover is file in folder - either jpg or png
    pub description: Option<TypedFile>, // description is file in folder - either txt, html, md
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub files: Vec<AudioFile>,
    pub subfolders: Vec<AudioFolderShort>,
}

impl SearchResult {
    pub fn new() -> Self {
        SearchResult {
            subfolders: vec![],
            files: vec![],
        }
    }
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

const DESCRIPTIONS: &[&str] = &["html", "plain", "x-markdown"];

pub fn is_description<P: AsRef<Path>>(path: P) -> bool {
    let mime = guess_mime_type(path);
    mime.type_() == "text" && has_subtype(&mime, DESCRIPTIONS)
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
