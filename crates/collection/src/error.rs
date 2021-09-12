pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Collection path is invalid")]
    InvalidCollectionPath,

    #[error("Db error")]
    DbError(#[from] sled::Error),

    #[error("Media metadata error")]
    MediaInfoError(#[from] media_info::Error),

    #[error("Invalid file name - not UTF8")]
    InvalidFileName,

    #[error("IO Error")]
    IOError(#[from] std::io::Error),

    #[error("Bincode serialization error")]
    BincodeError(#[from] Box<bincode::ErrorKind>),

    #[error("Missing Collection Cache: {0}")]
    MissingCollectionCache(String),
}
