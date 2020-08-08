use crate::util;
use std::ffi::{OsStr, OsString};
use std::path::Path;

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

pub fn is_existing_dir(p: &OsStr) -> Result<(), OsString> {
    let p = Path::new(p);
    if !p.is_dir() {
        return Err(format!("{:?} is not existing directory", p).into());
    }

    Ok(())
}

pub fn is_existing_file(p: &OsStr) -> Result<(), OsString> {
    let p = Path::new(p);
    if !p.is_file() {
        return Err(format!("{:?} is not existing file", p).into());
    }

    Ok(())
}

pub fn parent_dir_exists(p: &OsStr) -> Result<(), OsString> {
    if !util::parent_dir_exists(&p) {
        Err(format!("parent dir for {:?} does not exists", p).into())
    } else {
        Ok(())
    }
}

pub fn is_valid_url_path_prefix(s: String) -> ValidatorResult {
    if s.starts_with('/') && !s.ends_with('/') {
        Ok(())
    } else {
        Err("Must start with / but not end with it".into())
    }
}
