use std::path::{Path, PathBuf};
use std::fs;
use std::io;
use std::fs::read_link;
use std::fs::DirEntry;

use crate::config::get_config;
use super::types::*;
use super::audio_meta::{get_audio_properties, MediaInfo, Chapter};


fn os_to_string(s: ::std::ffi::OsString) -> String {
        match s.into_string() {
            Ok(s) => s,
            Err(s) => {
                warn!("Invalid file name - cannot covert to UTF8 : {:?}", s);
                "INVALID_NAME".into()
            }
        }
}

pub fn list_dir<P: AsRef<Path>, P2: AsRef<Path>>(
    base_dir: P,
    dir_path: P2,
) -> Result<AudioFolder, io::Error> {

   let full_path = base_dir.as_ref().join(&dir_path);
   match get_dir_type(&full_path)? {
       DirType::Dir => list_dir_dir(base_dir, full_path),
       DirType::File(chapters) => list_dir_file(base_dir, full_path, chapters),
       DirType::Other => Err(io::Error::new(io::ErrorKind::Other, "Not folder or chapterised audio file"))

   }
   

}

enum DirType {
    File(Vec<Chapter>),
    Dir,
    Other
}

fn get_dir_type<P:AsRef<Path>>(path: P) -> Result<DirType, io::Error> {
    let path = path.as_ref();
    let meta = if cfg!(feature="symlinks") {
        path.metadata()?
    } else {
        path.symlink_metadata()?
    };
    if meta.is_dir() {
        Ok(DirType::Dir)
    } else if meta.is_file() && is_audio(path) {
        let meta = get_audio_properties(&path).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        match meta.get_chapters() {
            Some(chapters) => Ok(DirType::File(chapters)),
            None => Ok(DirType::Other)
        }
    } else {
         Ok(DirType::Other)
    }
}

fn list_dir_file<P: AsRef<Path>>(
    base_dir: P,
    full_path: PathBuf,
    chapters: Vec<Chapter>
) -> Result<AudioFolder, io::Error> {
    unimplemented!()
}

fn list_dir_dir<P: AsRef<Path>>(
    base_dir: P,
    full_path: PathBuf,
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
                                subfolders.push(AudioFolderShort {
                                    path,
                                    name: os_to_string(f.file_name()),
                                    is_file: false
                                })
                            } else if ft.is_file() {
                                if is_audio(&path) {
                                    let mime = ::mime_guess::guess_mime_type(&path);
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
                                        subfolders.push(AudioFolderShort {
                                            path,
                                            name: os_to_string(f.file_name()),
                                            is_file: true
                                        })
                                    } else {
                                        files.push(AudioFile {
                                            meta: meta.get_audio_info(),
                                            path,
                                            name: os_to_string(f.file_name()),
                                            section: None,
                                            mime: mime.to_string(),
                                        });
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
                        full_path,
                        e
                    ),
                }
            }
            files.sort_unstable_by_key(|e| e.name.to_uppercase());
            subfolders.sort_unstable_by_key(|e| e.name.to_uppercase());;
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

#[cfg(feature = "symlinks")]
pub fn get_real_file_type<P: AsRef<Path>>(
    dir_entry: &DirEntry,
    full_path: P,
    allow_symlinks: bool,
) -> Result<::std::fs::FileType, io::Error> {
    let ft = dir_entry.file_type()?;

    if allow_symlinks && ft.is_symlink() {
        let p = read_link(dir_entry.path())?;
        let ap = if p.is_relative() {
            full_path.as_ref().join(p)
        } else {
            p
        };
        Ok(ap.metadata()?.file_type())
    } else {
        Ok(ft)
    }
}

#[cfg(not(feature = "symlinks"))]
pub fn get_real_file_type<P: AsRef<Path>>(
    dir_entry: &DirEntry,
    _full_path: P,
    _allow_symlinks: bool,
) -> Result<::std::fs::FileType, io::Error> {
    dir_entry.file_type()
}


#[cfg(test)]
mod tests {
    use super::*;
    use config::init_default_config;
    use serde_json;

    #[test]
    fn test_list_dir() {
        init_default_config();
        #[cfg(feature = "libavformat")]
        {
            media_info::init()
        }
        let res = list_dir("/non-existent", "folder");
        assert!(res.is_err());
        let res = list_dir("./", "test_data/");
        assert!(res.is_ok());
        let folder = res.unwrap();
        let num_media_files = if cfg!(feature = "libavformat") { 3 } else { 2 };
        assert_eq!(folder.files.len(), num_media_files);
        assert!(folder.cover.is_some());
        assert!(folder.description.is_some());
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
        let folder = list_dir("./", "test_data/").unwrap();
        let json = serde_json::to_string(&folder).unwrap();
        println!("JSON: {}", &json);
    }

    #[test]
    fn test_meta() {
        #[cfg(feature = "libavformat")]
        {
            media_info::init()
        }
        let path = Path::new("./test_data/01-file.mp3");
        let res = get_audio_properties(path);
        assert!(res.is_ok());
        let media_info = res.unwrap();
        let meta = media_info.get_audio_info().unwrap();
        assert_eq!(meta.bitrate, 220);
        assert_eq!(meta.duration, 2);
    }

}