use std::path::Path;

/// exists or is current dir
pub fn parent_dir_exists<P: AsRef<Path>>(p: &P) -> bool {
    match p.as_ref().parent() {
        Some(parent) => parent.as_os_str().is_empty() || parent.is_dir(),
        None => true,
    }
}

#[cfg(feature = "shared-positions")]
pub fn parse_cron<S: AsRef<str>>(exp: S) -> crate::error::Result<cron::Schedule> {
    let exp = format!("0 {} *", exp.as_ref());
    exp.parse().map_err(crate::Error::from)
}
