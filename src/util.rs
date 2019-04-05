use headers::Header;
use hyper::http::response::Builder;
use std::ops::{Bound,RangeBounds};
use std::cmp::{min, max};

pub fn os_to_string(s: ::std::ffi::OsString) -> String {
    match s.into_string() {
        Ok(s) => s,
        Err(s) => {
            warn!("Invalid file name - cannot covert to UTF8 : {:?}", s);
            "INVALID_NAME".into()
        }
    }
}

pub fn checked_dec(x: u64) -> u64 {
    if x > 0 {
        x - 1
    } else {
        x
    }
}

pub fn to_satisfiable_range<T:RangeBounds<u64>>(r:T, len: u64) -> Option<(u64,u64)> {

    match (r.start_bound(), r.end_bound()) {
        (Bound::Included(&start), Bound::Included(&end)) => if start <= end && start < len {
            Some((start, min(end, len-1)))
        } else {
            None
        }

        (Bound::Included(&start), Bound::Unbounded) => if start < len {
            Some((start, len-1))
        } else {
            None
        }

        (Bound::Unbounded, Bound::Included(&offset)) => if  offset > 0 {
            Some((max(len-offset, 0), len-1))
        } else {
            None
        }
        _ => None
    }

}

pub fn into_range_bounds(i: (u64,u64)) -> (Bound<u64>, Bound<u64>) {
    (Bound::Included(i.0), Bound::Included(i.1))
}

pub trait ResponseBuilderExt {
    fn typed_header<H: Header>(&mut self, header: H) -> &mut Builder;
}

impl ResponseBuilderExt for Builder {
    fn typed_header<H: Header>(&mut self, header: H) -> &mut Builder {
        let k = H::name();
        let mut values = vec![];
        header.encode(&mut values);
        for v in values.into_iter() {
            self.header(k, v);
        }
        self
    }
}
