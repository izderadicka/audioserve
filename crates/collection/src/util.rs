use std::fs::DirEntry;
use std::io;
use std::path::Path;

use mime_guess::Mime;

pub fn guess_mime_type<P: AsRef<Path>>(path: P) -> Mime {
    mime_guess::from_path(path).first_or_octet_stream()
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
