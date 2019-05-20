use std::ffi::{OsStr, OsString};
use std::path::Path;


type ValidatorResult = Result<(), String>;

pub fn is_socket_addr(v:String) -> ValidatorResult {
    let v = v.as_ref();
    if let Err(e) = str::parse::<std::net::SocketAddr>(v) {
                return Err(format!("{} is not socket address", v))
        };
    Ok(())
}

pub fn is_number(v:String) -> ValidatorResult {
    let v = v.as_ref();
    if let Err(e) = str::parse::<u32>(v) {
        return Err(format!("{} is not a number", v))
    }
    Ok(())
}

pub fn is_existing_dir(p: &OsStr) -> Result<(),OsString> {
    let p = Path::new(p);
    if ! p.is_dir() {
        return Err(format!("{:?} is not existing directory", p).into())
    }

    Ok(())
}

pub fn is_existing_file(p: &OsStr) -> Result<(),OsString> {
    let p = Path::new(p);
    if ! p.is_file() {
        return Err(format!("{:?} is not existing file", p).into())
    }

    Ok(())
}

pub fn parent_dir_exists(p: &OsStr) -> Result<(),OsString> {
   match Path::new(p).parent() {
       Some(p) => is_existing_dir(p.as_ref()),
        None => Ok(())
   }
}
