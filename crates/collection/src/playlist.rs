use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

use crate::audio_meta::is_audio;

const PL_EXTENTIONS: &[&str] = &["m3u", "m3u8"];

pub fn is_playlist(path: impl AsRef<Path>) -> bool {
    let path: &Path = path.as_ref();
    let ext = path.extension().and_then(|ext| ext.to_str());

    if let Some(ext) = ext {
        return PL_EXTENTIONS.contains(&ext);
    }

    false
}

fn validate_path(p: PathBuf) -> PlaylistItem {
    if p.is_absolute() {
        return PlaylistItem::Illegal(p);
    }

    let segments: Vec<_> = p.components().collect();
    let sz = segments.len();
    if sz > 4 {
        debug!("Too deep item in playlist");
        return PlaylistItem::Illegal(p);
    } else if sz < 1 {
        debug!("Empty playlist item");
        return PlaylistItem::Illegal(p);
    }

    if !segments.iter().all(|s| matches!(s, Component::Normal(_))) {
        debug!("Forbidden path segments like parent dir in playlist item");
        return PlaylistItem::Illegal(p);
    }

    if sz == 1 {
        PlaylistItem::CurrentDir(p)
    } else {
        let subdir = PathBuf::from(segments[0].as_os_str());
        PlaylistItem::Subdir(p, subdir)
    }
}

#[derive(Debug, Clone)]
enum PlaylistItem {
    CurrentDir(PathBuf),
    Subdir(PathBuf, PathBuf),
    Illegal(PathBuf),
}

impl From<PlaylistItem> for PathBuf {
    fn from(i: PlaylistItem) -> Self {
        match i {
            PlaylistItem::CurrentDir(d) => d,
            PlaylistItem::Subdir(d, _) => d,
            PlaylistItem::Illegal(d) => d,
        }
    }
}

impl AsRef<Path> for PlaylistItem {
    fn as_ref(&self) -> &Path {
        match self {
            PlaylistItem::CurrentDir(ref p) => p.as_path(),
            PlaylistItem::Subdir(p, _) => p.as_path(),
            PlaylistItem::Illegal(p) => p.as_path(),
        }
    }
}

struct PlaylistIterator<T, B> {
    lines: io::Lines<BufReader<T>>,
    base_path: B,
}
impl<B: AsRef<Path>> PlaylistIterator<File, B> {
    fn from_file(f: impl AsRef<Path>, base_path: B) -> Result<Self, io::Error> {
        let pl = PlaylistIterator {
            lines: BufReader::new(File::open(f)?).lines(),
            base_path, //items: Vec::new(),
        };
        Ok(pl)
    }
}

#[cfg(test)]
impl<T: Read, B: AsRef<Path>> PlaylistIterator<T, B> {
    fn new(reader: T, base_path: B) -> Self {
        PlaylistIterator {
            lines: BufReader::new(reader).lines(),
            base_path,
        }
    }
}

impl<T: Read, B: AsRef<Path>> Iterator for PlaylistIterator<T, B> {
    type Item = PlaylistItem;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(maybe_line) = self.lines.next() {
            match maybe_line {
                Ok(line) => {
                    let line = line.trim();
                    if line.starts_with('#') || line.is_empty() {
                        continue;
                    } else {
                        let rel_path = PathBuf::from(line);

                        let item = validate_path(rel_path);
                        if matches!(item, PlaylistItem::Illegal(_)) {
                            debug!("Invalid path in playlist {:?}", item.as_ref());
                            continue;
                        }
                        let full_path = self.base_path.as_ref().join(&item);
                        if !(full_path.is_file() && is_audio(&item)) {
                            debug!(
                                "Non existent or not audio playlist item {:?}",
                                item.as_ref()
                            );
                            continue;
                        }
                        return Some(item);
                    }
                }
                Err(_) => return None,
            }
        }
        None
    }
}

pub struct Playlist {
    items: Vec<PathBuf>,
    covered_dirs: HashSet<PathBuf>,
    path: PathBuf,
}

impl Playlist {
    pub fn new(file: impl Into<PathBuf>, base_path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let mut covered_dirs = HashSet::new();
        let path: PathBuf = file.into();
        let items = PlaylistIterator::from_file(&path, base_path)?
            .map(|i| match i {
                PlaylistItem::CurrentDir(p) => p,
                PlaylistItem::Subdir(p, subdir) => {
                    covered_dirs.insert(subdir);
                    p
                }
                PlaylistItem::Illegal(_) => unreachable!(),
            })
            .collect();

        Ok(Playlist {
            items,
            covered_dirs,
            path,
        })
    }

    pub fn to_items(self) -> Vec<PathBuf> {
        self.items
    }

    pub fn is_covering(&self, p: &Path) -> bool {
        self.covered_dirs.contains(p)
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn has_subfolders(&self) -> bool {
        !self.covered_dirs.is_empty()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_detection() {
        assert!(is_playlist("mp3tag.m3u"));
        assert!(!is_playlist("track.mp3"));
    }

    #[test]
    fn test_iterator() {
        let base_path = PathBuf::from("../../test_data");
        println!("Current dir is {:?}", std::env::current_dir().unwrap());
        assert!(base_path.is_dir());
        let pl = PlaylistIterator::new(
            "01-file.mp3\n02-file.opus\n03-file.mka".as_bytes(),
            &base_path,
        );
        let res: Vec<_> = pl.collect();
        assert_eq!(3, res.len());
        assert_eq!("03-file.mka", res[2].as_ref().to_str().unwrap())
    }

    #[test]
    fn test_playlist() {
        let base_path = PathBuf::from("../../test_data");
        let pl = Playlist::new(base_path.join("playlist.m3u"), &base_path).unwrap();
        assert!(pl.is_covering(Path::new("usak")));
        let items = pl.to_items();
        assert_eq!(4, items.len());
    }
}
