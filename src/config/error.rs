use std::borrow::Cow;
use std::fmt::{self, Display};

#[derive(Debug)]
pub enum ErrorKind {
    Argument {
        argument: &'static str,
        message: Cow<'static, str>,
    },
    ConfigValue {
        name: &'static str,
        message: Cow<'static, str>,
    },
}

#[derive(Debug)]
pub struct Error(ErrorKind);

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            ErrorKind::Argument {
                argument,
                ref message,
            } => write!(f, "Error in argument {}: {}", argument, message),
            ErrorKind::ConfigValue { name, ref message } => {
                write!(f, "Error in config value {}: {}", name, message)
            }
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn in_argument<T, S>(argument: &'static str, msg: S) -> std::result::Result<T, Self>
    where
        S: Into<Cow<'static, str>>,
    {
        Err(Error(ErrorKind::Argument {
            argument,
            message: msg.into(),
        }))
    }

    pub fn in_value<T, S>(name: &'static str, msg: S) -> std::result::Result<T, Self>
    where
        S: Into<Cow<'static, str>>,
    {
        Err(Error(ErrorKind::ConfigValue {
            name,
            message: msg.into(),
        }))
    }
}

macro_rules!  value_error {
    ($arg:expr, $msg:expr) => {
        Error::in_value($arg, $msg)
    };

    ($arg:expr, $msg:expr, $($param:expr),+) => {
        Error::in_value($arg,
        format!($msg, $($param),+))
    };

}