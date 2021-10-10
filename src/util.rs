use headers::{Header, HeaderMapExt};
use hyper::http::response::Builder;
use std::cmp::{max, min};
use std::{
    ops::{Bound, RangeBounds},
    path::Path,
};

pub fn parent_dir_exists<P: AsRef<Path>>(p: &P) -> bool {
    match p.as_ref().parent() {
        Some(parent) => !(!parent.as_os_str().is_empty() && !parent.is_dir()),
        None => true,
    }
}

pub fn checked_dec(x: u64) -> u64 {
    if x > 0 {
        x - 1
    } else {
        x
    }
}

pub fn to_satisfiable_range<T: RangeBounds<u64>>(r: T, len: u64) -> Option<(u64, u64)> {
    match (r.start_bound(), r.end_bound()) {
        (Bound::Included(&start), Bound::Included(&end)) => {
            if start <= end && start < len {
                Some((start, min(end, len - 1)))
            } else {
                None
            }
        }

        (Bound::Included(&start), Bound::Unbounded) => {
            if start < len {
                Some((start, len - 1))
            } else {
                None
            }
        }

        (Bound::Unbounded, Bound::Included(&offset)) => {
            if offset > 0 {
                Some((max(len - offset, 0), len - 1))
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn into_range_bounds(i: (u64, u64)) -> (Bound<u64>, Bound<u64>) {
    (Bound::Included(i.0), Bound::Included(i.1))
}

pub fn header2header<H1: Header, H2: Header>(i: H1) -> Result<impl Header, headers::Error> {
    let mut v = vec![];
    i.encode(&mut v);
    H2::decode(&mut v.iter())
}

pub trait ResponseBuilderExt {
    fn typed_header<H: Header>(self, header: H) -> Self;
}

impl ResponseBuilderExt for Builder {
    fn typed_header<H: Header>(mut self, header: H) -> Builder {
        if let Some(h) = self.headers_mut() {
            h.typed_insert(header)
        };
        self
    }
}

/// Checks whether the pattern matches at the front of the haystack.
#[inline]
fn is_prefix_of(needle: &str, haystack: &str) -> bool {
    haystack.as_bytes().starts_with(needle.as_bytes())
}

/// Removes the pattern from the front of haystack, if it matches.
#[inline]
pub fn strip_prefix_of<'a>(needle: &str, haystack: &'a str) -> Option<&'a str> {
    if is_prefix_of(needle, haystack) {
        // SAFETY: prefix was just verified to exist.
        unsafe { Some(haystack.get_unchecked(needle.as_bytes().len()..)) }
    } else {
        None
    }
}
