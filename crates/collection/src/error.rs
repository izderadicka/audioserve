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

    #[error("Invalid file name - not UTF8")]
    InvalidFileName,

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
}

impl From<TransactionError<Error>> for Error {
    fn from(e: TransactionError<Error>) -> Self {
        match e {
            TransactionError::Abort(e) => e,
            TransactionError::Storage(e) => Error::DbError(e),
        }
    }
}
