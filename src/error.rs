//! this module just reexport anyhow to enable simple indirection to possibly change
//! error implementation
pub use anyhow::{Error, Result, Context, bail};
