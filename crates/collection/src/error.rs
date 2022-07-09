use std::path::StripPrefixError;

use sled::transaction::TransactionError;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Collection path is invalid")]
    InvalidCollectionPath,

    #[error("Db error: {0}")]
    DbError(#[from] sled::Error),

    #[error("Db transaction error: {0}")]
    DbTransactionError(String),

    #[error("Media metadata error: {0}")]
    MediaInfoError(#[from] media_info::Error),

    #[error("Invalid path - not UTF8")]
    InvalidPath,

    #[error("Invalid path: {0}")]
    InvalidPathNonUtf(#[from] std::str::Utf8Error),

    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Bincode serialization error: {0}")]
    BincodeError(#[from] Box<bincode::ErrorKind>),

    #[error("Missing Collection Cache: {0}")]
    MissingCollectionCache(usize),

    #[cfg(feature = "async")]
    #[error("Tokio join error: {0}")]
    TokioJoinError(#[from] tokio::task::JoinError),

    #[error("Too many position groups")]
    TooManyGroups,

    #[error("Invalid path: {0}")]
    InvalidPathPrefix(#[from] StripPrefixError),

    #[error("Position cannot be inserted")]
    IgnoredPosition,

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("JSON schema error: {0}")]
    JsonSchemaError(String),

    #[error("JSON schema error: {0}")]
    JsonDataError(String),

    #[error("Invalid collection option: {0}")]
    InvalidCollectionOption(String),

    #[error("Invalid regex for CD folder: {0} {1}")]
    InvalidCDFolderRegex(String, regex::Error),
}

macro_rules! invalid_option_err {
    ($fmt: literal, $($param: expr),*) => {
       crate::error::Error::InvalidCollectionOption(format!($fmt, $($param),*))
    };
}

macro_rules! invalid_option {
    ($fmt: literal, $($param: expr),*) => {
        return Err(invalid_option_err!($fmt, $($param),*))
    };
}

pub(crate) use invalid_option;
pub(crate) use invalid_option_err;

impl From<TransactionError<Error>> for Error {
    fn from(e: TransactionError<Error>) -> Self {
        match e {
            TransactionError::Abort(e) => e,
            TransactionError::Storage(e) => Error::DbError(e),
        }
    }
}
