use std::borrow::Cow;
use thiserror::Error;

#[derive(Error,Debug)]
pub enum Error {
    #[error("Error in argument {argument}: {message}")]
    Argument {
        argument: &'static str,
        message: Cow<'static, str>,
    },

    #[error("Error in config value {name}: {message}")]
    ConfigValue {
        name: &'static str,
        message: Cow<'static, str>,
    },
}


pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn in_argument<T, S>(argument: &'static str, msg: S) -> std::result::Result<T, Self>
    where
        S: Into<Cow<'static, str>>,
    {
        Err(Error::Argument {
            argument,
            message: msg.into(),
        })
    }

    pub fn in_value<T, S>(name: &'static str, msg: S) -> std::result::Result<T, Self>
    where
        S: Into<Cow<'static, str>>,
    {
        Err(Error::ConfigValue {
            name,
            message: msg.into(),
        })
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
