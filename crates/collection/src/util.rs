use std::ffi::OsStr;
use std::fs::{DirEntry, Metadata};
use std::path::Path;
use std::time::SystemTime;
use std::{io, thread};

use mime_guess::Mime;

pub fn guess_mime_type<P: AsRef<Path>>(path: P) -> Mime {
    mime_guess::from_path(path).first_or_octet_stream()
}

pub fn get_file_name<P: AsRef<Path>>(path: P) -> String {
    path.as_ref()
        .file_name()
        .unwrap_or_else(|| OsStr::new("<UNKNOWN>"))
        .to_string_lossy()
        .into()
}

#[cfg(feature = "symlinks")]
pub fn get_meta<P: AsRef<Path>>(path: P) -> Result<Metadata, io::Error> {
    let path = path.as_ref();
    path.metadata()
}

#[cfg(not(feature = "symlinks"))]
pub fn get_meta<P: AsRef<Path>>(path: P) -> Result<Metadata, io::Error> {
    let path = path.as_ref();
    path.symlink_metadata()
}

#[cfg(feature = "symlinks")]
pub fn get_modified<P: AsRef<Path>>(path: P) -> Option<SystemTime> {
    let path = path.as_ref();
    // TODO: use is_symlink when it becomes stable
    let mod1 = path.symlink_metadata().and_then(|m| m.modified());
    let mod2 = path.metadata().and_then(|m| m.modified());
    match (mod1, mod2) {
        (Ok(m1), Ok(m2)) => Some(m1.max(m2)),
        _ => {
            // everything else is problem, so rather do not rely on mod time
            warn!("Error getting modtime for {:?}", path);
            None
        }
    }
}

#[cfg(not(feature = "symlinks"))]
pub fn get_modified<P: AsRef<Path>>(path: P) -> Option<SystemTime> {
    let path = path.as_ref();
    path.symlink_metadata().and_then(|op| op.modified()).ok()
}

#[cfg(feature = "symlinks")]
pub fn get_real_file_type<P: AsRef<Path>>(
    dir_entry: &DirEntry,
    full_path: P,
    allow_symlinks: bool,
) -> Result<::std::fs::FileType, io::Error> {
    let ft = dir_entry.file_type()?;

    if allow_symlinks && ft.is_symlink() {
        let p = std::fs::read_link(dir_entry.path())?;
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

pub fn spawn_named_thread<F, T>(name: impl Into<String>, f: F) -> thread::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let builder = thread::Builder::new().name(name.into()); //TODO: consider setting stack size 1M
    builder.spawn(f).expect("Invalid thread name")
}
