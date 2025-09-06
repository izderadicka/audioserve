use std::path::{Path, PathBuf};

use crate::{audio_meta::AudioFolder, AudioFolderShort};

pub fn update_path(
    from: &Path,
    to: &Path,
    p: &Path,
) -> std::result::Result<PathBuf, std::path::StripPrefixError> {
    let p = p.strip_prefix(from)?;
    //Unfortunatelly join adds traling slash if joined path is empty, which causes problem, so we need to handle this special case
    if p.as_os_str().is_empty() {
        return Ok(to.into());
    }
    Ok(to.join(p))
}

pub fn deser_audiofolder<T: AsRef<[u8]>>(data: T) -> Option<AudioFolder> {
    crate::deserialize(data.as_ref())
        .map_err(|e| error!("Error deserializing data from db {}", e))
        .ok()
}

pub fn kv_to_audiofolder<K: AsRef<str>, V: AsRef<[u8]>>(key: K, val: V) -> AudioFolderShort {
    let path = Path::new(key.as_ref());
    let folder = deser_audiofolder(val);
    AudioFolderShort {
        name: path.file_name().unwrap().to_string_lossy().into(),
        path: path.into(),
        is_file: folder.as_ref().map(|f| f.is_file).unwrap_or(false),
        modified: folder.as_ref().and_then(|f| f.modified),
        finished: false,
    }
}

pub fn parent_path<P: AsRef<Path>>(path: P) -> PathBuf {
    path.as_ref()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default()
}

pub fn split_path<S: AsRef<str>>(p: &S) -> (&str, &str) {
    let s = p.as_ref();
    match s.rsplit_once('/') {
        Some((path, file)) => (path, file),
        None => ("", s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parent_path() {
        let p1 = Path::new("usak/kulisak");
        assert_eq!(Path::new("usak"), parent_path(p1));
        let p2 = Path::new("usak");
        assert_eq!(Path::new(""), parent_path(p2));
    }
}
