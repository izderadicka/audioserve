use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Year ${0} is not zip/DOS range 1980 - 2107")]
    InvalidYear(i32),
    #[error("File is too big {0}")]
    FileTooBig(u64),
    #[error("File name is too big (bigger then 65535)")]
    FileNameTooBig,
    #[error("ZIP Archive too big (over 4GB)")]
    ArchiveTooBig,
    #[error("IO error ${0}")]
    Io(#[from] io::Error),
    #[error("Invalid path - does not contain file name")]
    InvalidPath,
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Io(e) => e,
            other => io::Error::new(io::ErrorKind::Other, other),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
