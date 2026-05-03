use anyhow::{bail, Context};

use crate::util;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use super::PositionsBackupFormat;

pub fn is_existing_dir(p: &str) -> Result<PathBuf, anyhow::Error> {
    let p = Path::new(p);
    if !p.is_dir() {
        bail!("{:?} is not existing directory", p);
    }

    Ok(p.into())
}

pub fn is_existing_file(p: &str) -> Result<PathBuf, anyhow::Error> {
    let p = Path::new(p);
    if !p.is_file() {
        bail!("{:?} is not existing file", p);
    }

    Ok(p.into())
}

pub fn parent_dir_exists(p: &str) -> Result<PathBuf, anyhow::Error> {
    if !util::parent_dir_exists(&p) {
        bail!("parent dir for {:?} does not exists", p);
    } else {
        Ok(Path::new(p).into())
    }
}

pub fn is_valid_url_path_prefix(s: &str) -> Result<String, anyhow::Error> {
    if s.starts_with('/') && !s.ends_with('/') {
        Ok(s.into())
    } else {
        bail!("Must start with / but not end with it");
    }
}

pub fn duration_secs(s: &str) -> Result<Duration, anyhow::Error> {
    let secs: u64 = s.parse().context("Invalid Duration")?;
    Ok(Duration::from_secs(secs))
}

pub fn positions_restore_format(s: &str) -> Result<PositionsBackupFormat, anyhow::Error> {
    let format: PositionsBackupFormat =
        s.parse().context(format!("Invalid format string {}", s))?;
    Ok(format)
}

#[cfg(test)]
mod tests {
    use super::is_valid_url_path_prefix;

    #[test]
    fn test_valid_prefix() {
        assert!(is_valid_url_path_prefix("/api").is_ok());
        assert!(is_valid_url_path_prefix("/api/v1").is_ok());
        assert_eq!("/myapp", is_valid_url_path_prefix("/myapp").unwrap());
    }

    #[test]
    fn test_invalid_prefix_root_only() {
        // "/" both starts and ends with '/' → rejected
        assert!(is_valid_url_path_prefix("/").is_err());
    }

    #[test]
    fn test_invalid_prefix_no_leading_slash() {
        assert!(is_valid_url_path_prefix("api").is_err());
        assert!(is_valid_url_path_prefix("api/v1").is_err());
    }

    #[test]
    fn test_invalid_prefix_trailing_slash() {
        assert!(is_valid_url_path_prefix("/api/").is_err());
        assert!(is_valid_url_path_prefix("/api/v1/").is_err());
    }
}
