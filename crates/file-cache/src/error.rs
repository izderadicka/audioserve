use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("key {0} exists")]
    KeyAlreadyExists(String),

    #[error("key is invalid - too big")]
    InvalidKey,

    #[error("index file is invalid")]
    InvalidIndex,

    #[error("file bigger then max cache size")]
    FileTooBig,

    #[error("key {0} is being added")]
    KeyOpened(String),

    #[error("invalid cache state: {0}")]
    InvalidCacheState(String),

    #[error("Error when running async task")]
    Executor,
}
