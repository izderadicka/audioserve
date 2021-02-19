use crate::error::{Error, Result};
use chrono::{DateTime, Datelike, Local, Timelike};
use std::time::SystemTime;

pub struct Timestamp(DateTime<Local>);

impl Timestamp {
    // pub fn local(&self) -> DateTime<Local> {
    //     self.0.into()
    // }

    pub fn dos_timepart(&self) -> u16 {
        let t = self.0.time();
        ((t.second() as u16) >> 1) | ((t.minute() as u16) << 5) | ((t.hour() as u16) << 11)
    }

    pub fn dos_datepart(&self) -> Result<u16> {
        let d = self.0.date();
        if d.year() < 1980 || d.year() > 2107 {
            return Err(Error::InvalidYear(d.year()));
        }
        Ok((d.day() as u16) | ((d.month() as u16) << 5) | (((d.year() - 1980) as u16) << 9))
    }
}

impl From<SystemTime> for Timestamp {
    fn from(t: SystemTime) -> Self {
        Timestamp(t.into())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Datelike;

    use super::*;

    #[test]
    fn test_sys_time() {
        let dt: DateTime<Local> = SystemTime::now().into();
        let dt2: DateTime<Local> = SystemTime::now().into();

        assert_eq!(dt.year(), dt2.year());
    }
}
