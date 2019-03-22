use std::sync::{Arc, Condvar, Mutex};
use std::fs::{DirEntry, read_link};
use std::io;

#[derive(Clone)]
pub(crate) struct Cond(Arc<(Mutex<bool>, Condvar)>);
impl Cond {
    pub fn new() -> Self {
        Cond(Arc::new((Mutex::new(false), Condvar::new())))
    }
    
    pub fn notify(&self) {
        let mut x = (self.0).0.lock().unwrap();
        *x = true;
        (self.0).1.notify_one();
    }
    
    pub fn wait(&self) {
        let mut x = (self.0).0.lock().unwrap();
        while !*x {
            x = (self.0).1.wait(x).unwrap();
        }
        *x = false;
    }
}

#[cfg(feature = "symlinks")]
pub fn get_real_file_type(
    dir_entry: &DirEntry,
    allow_symlinks: bool,
) -> Result<::std::fs::FileType, io::Error> {
    let ft = dir_entry.file_type()?;

    if allow_symlinks && ft.is_symlink() {
        let p = read_link(dir_entry.path())?;
        let ap = if p.is_relative() {
            dir_entry.path().parent().unwrap().join(p)
        } else {
            p
        };
        Ok(ap.metadata()?.file_type())
    } else {
        Ok(ft)
    }
}

#[cfg(not(feature = "symlinks"))]
pub fn get_real_file_type(
    dir_entry: &DirEntry,
    _allow_symlinks: bool,
) -> Result<::std::fs::FileType, io::Error> {
    dir_entry.file_type()
}
