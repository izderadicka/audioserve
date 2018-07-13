use std::error::Error as StdErr;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub struct Error(Option<Box<dyn StdErr + Send + Sync>>);

impl Display for Error {
    fn fmt(&self, fmt: &mut Formatter) -> ::std::result::Result<(), ::std::fmt::Error> {
        fmt.write_str("Audioserve error")?;
        if let Some(e) = self.cause() {
            write!(fmt, "\nCause: {}", e)
        } else {
            Ok(())
        }
    }
}

impl StdErr for Error {
    fn cause(&self) -> Option<&StdErr> {
        self.0.as_ref().map(|e| e.as_ref() as &StdErr)
    }
}

impl Error {
    pub fn new() -> Self {
        Error(None)
    }

    pub fn new_with_cause<E: StdErr + Send + Sync + 'static>(cause: E) -> Self {
        Error(Some(Box::new(cause)))
    }
}
