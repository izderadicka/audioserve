use sled::transaction::TransactionError;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Collection path is invalid")]
    InvalidCollectionPath,

    #[error("Db error")]
    DbError(#[from] sled::Error),

    #[error("Db transaction error")]
    DbTransactionError(String),

    #[error("Media metadata error")]
    MediaInfoError(#[from] media_info::Error),

    #[error("Invalid file name - not UTF8")]
    InvalidFileName,

    #[error("IO Error")]
    IOError(#[from] std::io::Error),

    #[error("Bincode serialization error")]
    BincodeError(#[from] Box<bincode::ErrorKind>),

    #[error("Missing Collection Cache: {0}")]
    MissingCollectionCache(usize),
}

impl From<TransactionError<Error>> for Error {
    fn from(e: TransactionError<Error>) -> Self {
        match e {
            TransactionError::Abort(e) => e,
            TransactionError::Storage(e) => Error::DbError(e),
        }
    }
}
