use config::get_config;
use mime::Mime;
use mime_guess::guess_mime_type;
use services::transcode::{TranscodingFormat, QualityLevel};
use std::path::{Path, PathBuf};

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
pub struct AudioFile {
    pub name: String,
    pub path: PathBuf,
    pub meta: Option<AudioMeta>,
    pub mime: String,
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AudioMeta {
    pub duration: u32, // duration in seconds, if available
    pub bitrate: u32,  // bitrate in kB/s
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AudioFolderShort {
    pub name: String,
    pub path: PathBuf,
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
    name: &'static str
}

impl From<TranscodingFormat> for TranscodingSummary {
    fn from(f: TranscodingFormat) -> Self {
        TranscodingSummary {
            bitrate: f.bitrate(),
            name: f.format_name()
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Transcodings {
    pub max_transcodings: usize,
    pub low: TranscodingSummary,
    pub medium: TranscodingSummary,
    pub high: TranscodingSummary,
}

impl Transcodings {
    pub fn new() -> Self {
        let cfg = get_config();
        Transcodings {
            max_transcodings: cfg.max_transcodings,
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
