use anyhow::bail;

use crate::util;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

type ValidatorResult = Result<(), String>;

pub fn is_socket_addr(v: String) -> ValidatorResult {
    let v = v.as_ref();
    if str::parse::<std::net::SocketAddr>(v).is_err() {
        return Err(format!("{} is not socket address", v));
    };
    Ok(())
}

pub fn is_number(v: String) -> ValidatorResult {
    let v = v.as_ref();
    if str::parse::<u32>(v).is_err() {
        return Err(format!("{} is not a number", v));
    }
    Ok(())
}

pub fn is_positive_float(v: String) -> ValidatorResult {
    let x: f32 = v.parse().map_err(|_| format!("{} is not a number", v))?;
    if x < 1e-18 {
        return Err("Number should be bigger then 0 at least by small bit".into());
    }
    Ok(())
}

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
