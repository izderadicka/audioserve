//! this module just reexport anyhow to enable simple indirection to possibly change
//! error implementation
pub use anyhow::{bail, Context, Error, Result};
