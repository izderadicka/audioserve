use std::path::{PathBuf, Path};
use mime::Mime;
use mime_guess::{guess_mime_type};

#[derive(Debug,Serialize)]
pub struct TypedFile {
    pub path: PathBuf,
    pub mime: String
}

impl TypedFile {
    pub fn new<P:Into<PathBuf>>(path:P) -> Self {
        let path = path.into();
        let mime = guess_mime_type(&path);
        TypedFile{path, mime:mime.as_ref().into()}
    }
}

#[derive(Debug,Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AudioFile {
    pub name: String,
    pub path: PathBuf
}


#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AudioFolderShort {
    pub name: String,
    pub path: PathBuf
}


#[derive(Debug,Serialize)]
pub struct AudioFolder {
    pub files: Vec<AudioFile>,
    pub subfolders: Vec<AudioFolderShort>,
    pub cover: Option<TypedFile>, // cover is file in folder - either jpg or png
    pub description: Option<TypedFile> // description is file in folder - either txt, html, md
}

fn has_subtype(mime: &Mime, subtypes: &[&str]) -> bool {
    subtypes.iter().find(|&&s| s==mime.subtype()).is_some()
}


pub fn is_audio<P: AsRef<Path>>(path:P) -> bool {
    let mime= guess_mime_type(path);
    mime.type_() == "audio"
}

const COVERS: &'static [&'static str] = & ["jpeg", "png"];

pub fn is_cover<P: AsRef<Path>>(path:P) -> bool {
    let mime = guess_mime_type(path);
    mime.type_() == "image" && has_subtype(&mime, COVERS)
}

const DESCRIPTIONS: &'static [&'static str] = & ["html", "plain", "x-markdown"];

pub fn is_description<P: AsRef<Path>>(path:P) -> bool {
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
        assert!( ! is_audio("cover.jpg"));
    }

    #[test]
    fn test_is_cover() {
        assert!(is_cover("cover.jpg"));
        assert!(! is_cover("my/song.mp3"));
    }

    #[test]
    fn test_is_description() {
        assert!(!is_description("cover.jpg"));
        assert!(is_description("about.html"));
        assert!(is_description("about.txt"));
        assert!(is_description("some/folder/text.md"));
    }
}