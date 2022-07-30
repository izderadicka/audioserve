#[cfg(feature = "alternate-encoding")]
use encoding::{label::encoding_from_whatwg_label, DecoderTrap, EncodingRef};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::slice;
use thiserror::Error;

#[allow(dead_code)]
#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(deref_nullptr)]
mod ffi;
pub mod tags;

#[derive(Error, Debug)]
pub enum Error {
    #[error("libav error code {0}")]
    AVError(i32),
    #[error("memory allocation error - maybe full memory")]
    AllocationError,

    #[error("UTF8 error: {0}")]
    InvalidString(#[from] std::str::Utf8Error),

    #[error("Invalid encoding name {0}")]
    InvalidEncoding(String),
}

// fn string_from_ptr(ptr: *const c_char) -> Result<Option<String>> {
//     if ptr.is_null() {
//         Ok(None)
//     } else {
//         unsafe { Ok(Some(CStr::from_ptr(ptr).to_str()?.to_owned())) }
//     }
// }
#[cfg(not(feature = "alternate-encoding"))]
fn string_from_ptr_lossy(ptr: *const c_char) -> String {
    let data = unsafe { CStr::from_ptr(ptr) }.to_bytes();
    String::from_utf8_lossy(data).into()
}

#[cfg(feature = "alternate-encoding")]
fn string_from_ptr_lossy(ptr: *const c_char, alternate_encoding: Option<EncodingRef>) -> String {
    if ptr.is_null() {
        "".into()
    } else {
        let data = unsafe { CStr::from_ptr(ptr) }.to_bytes();
        std::str::from_utf8(data)
            .ok()
            .map(|s| s.to_string())
            .or_else(|| alternate_encoding.and_then(|e| e.decode(data, DecoderTrap::Strict).ok()))
            .unwrap_or_else(|| String::from_utf8_lossy(data).into())
    }
}

struct Dictionary {
    pub dic: *mut ffi::AVDictionary,
    #[cfg(feature = "alternate-encoding")]
    pub alternate_encoding: Option<EncodingRef>,
}

impl Dictionary {
    fn new(dic: *mut ffi::AVDictionary) -> Self {
        #[cfg(feature = "alternate-encoding")]
        return Dictionary {
            dic,
            alternate_encoding: None,
        };
        #[cfg(not(feature = "alternate-encoding"))]
        return Dictionary { dic };
    }

    #[cfg(feature = "alternate-encoding")]
    fn new_with_encoding(
        dic: *mut ffi::AVDictionary,
        encoding_name: impl AsRef<str>,
    ) -> Result<Self> {
        let encoding = encoding_from_whatwg_label(encoding_name.as_ref())
            .ok_or_else(|| Error::InvalidEncoding(encoding_name.as_ref().to_string()))?;

        Ok(Dictionary {
            dic,
            alternate_encoding: Some(encoding),
        })
    }

    fn get<S: AsRef<str>>(&self, key: S) -> Option<String> {
        if self.dic.is_null() {
            return None;
        }
        let cs = CString::new(key.as_ref()).expect("zero byte in key");
        let res = unsafe { ffi::av_dict_get(self.dic, cs.as_ptr(), ptr::null(), 0) };
        if res.is_null() {
            return None;
        }

        #[cfg(feature = "alternate-encoding")]
        {
            Some(string_from_ptr_lossy(
                (unsafe { *res }).value,
                self.alternate_encoding,
            ))
        }
        #[cfg(not(feature = "alternate-encoding"))]
        {
            Some(string_from_ptr_lossy((unsafe { *res }).value))
        }
    }

    fn get_all(&self) -> HashMap<String, String> {
        let empty = CString::new("").unwrap();
        let mut map = HashMap::new();
        let mut prev = ptr::null();
        loop {
            let current = unsafe {
                ffi::av_dict_get(
                    self.dic,
                    empty.as_ptr(),
                    prev,
                    ffi::AV_DICT_IGNORE_SUFFIX as i32,
                )
            };
            if current.is_null() {
                break;
            } else {
                #[cfg(feature = "alternate-encoding")]
                let (key, value) = (
                    string_from_ptr_lossy((unsafe { *current }).key, self.alternate_encoding),
                    string_from_ptr_lossy((unsafe { *current }).value, self.alternate_encoding),
                );
                #[cfg(not(feature = "alternate-encoding"))]
                let (key, value) = (
                    string_from_ptr_lossy((unsafe { *current }).key),
                    string_from_ptr_lossy((unsafe { *current }).value),
                );

                map.insert(key, value);
                prev = current;
            }
        }

        map
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn check_ret(res: i32) -> Result<()> {
    if res == 0 {
        Ok(())
    } else {
        Err(Error::AVError(res))
    }
}

pub fn init() {
    unsafe {
        ffi::av_log_set_level(ffi::AV_LOG_QUIET);
        ffi::av_register_all()
    }
}

pub fn version() -> u32 {
    unsafe { ffi::avformat_version() }
}

#[derive(Debug, Clone)]
pub struct Chapter {
    pub title: String,
    pub num: i32,
    pub start: u64,
    pub end: u64,
}

pub struct MediaFile {
    ctx: *mut ffi::AVFormatContext,
    meta: Dictionary,
}

macro_rules! meta_methods {
    ($self:ident $( $name:ident )+) => {
        $(
        pub fn $name(&$self) -> Option<String> {
        $self.meta(stringify!($name))
        }
        )+
    };
}

impl MediaFile {
    pub fn open<S: AsRef<str>>(fname: S) -> Result<Self> {
        MediaFile::prepare_open(fname).map(|(ctx, m)| MediaFile {
            ctx,
            meta: Dictionary::new(m),
        })
    }

    fn prepare_open(
        fname: impl AsRef<str>,
    ) -> Result<(*mut ffi::AVFormatContext, *mut ffi::AVDictionary)> {
        unsafe {
            let mut ctx = ffi::avformat_alloc_context();
            assert!(ctx as usize > 0);
            //(*ctx).probesize = 5*1024*1024*1024;
            let name = CString::new(fname.as_ref()).unwrap();
            let ret =
                ffi::avformat_open_input(&mut ctx, name.as_ptr(), ptr::null_mut(), ptr::null_mut());
            check_ret(ret)?;
            if ctx.is_null() {
                return Err(Error::AllocationError);
            }
            let ret = ffi::avformat_find_stream_info(ctx, ptr::null_mut());
            check_ret(ret)?;

            let mut m = (*ctx).metadata;

            if ffi::av_dict_count(m) == 0 && (*ctx).nb_streams > 0 {
                //OK we do not have meta in main header, let's look at streams

                let streams = slice::from_raw_parts((*ctx).streams, (*ctx).nb_streams as usize);

                for s in streams {
                    let codec_type = (*(**s).codecpar).codec_type;
                    if codec_type == ffi::AVMediaType_AVMEDIA_TYPE_AUDIO {
                        m = (**s).metadata;
                        break;
                    }
                }
            }

            Ok((ctx, m))
        }
    }

    #[cfg(feature = "alternate-encoding")]
    pub fn open_with_encoding<S: AsRef<str>>(
        fname: S,
        alternate_encoding: Option<impl AsRef<str>>,
    ) -> Result<Self> {
        MediaFile::prepare_open(fname).and_then(|(ctx, m)| {
            let meta = match alternate_encoding {
                Some(e) => Dictionary::new_with_encoding(m, e)?,
                None => Dictionary::new(m),
            };
            Ok(MediaFile { ctx, meta })
        })
    }

    /// Duration in ms
    pub fn duration(&self) -> u64 {
        let d = unsafe { (*self.ctx).duration } / 1_000;
        if d < 0 {
            0
        } else {
            d as u64
        }
    }

    /// bitrate in kbps
    pub fn bitrate(&self) -> u32 {
        let b = unsafe { (*self.ctx).bit_rate } / 1000;

        if b < 0 {
            0
        } else {
            b as u32
        }
    }
    meta_methods!(self title album artist composer genre track  );

    pub fn meta<S: AsRef<str>>(&self, key: S) -> Option<String> {
        self.meta.get(&key)
        //.or_else(|| self.meta.get(key.as_ref().to_uppercase()))
    }

    pub fn all_meta(&self) -> HashMap<String, String> {
        self.meta.get_all()
    }

    pub fn chapters_count(&self) -> usize {
        unsafe { (*self.ctx).nb_chapters as usize }
    }

    pub fn chapters(&self) -> Option<Vec<Chapter>> {
        fn norm_time(t: i64, time_base: ffi::AVRational) -> u64 {
            assert!(t >= 0);
            t as u64 * 1000 * time_base.num as u64 / time_base.den as u64
        }
        unsafe {
            let num_chapters = (*self.ctx).nb_chapters as usize;
            if num_chapters == 0 {
                return None;
            }
            let mut c = Vec::new();
            let chaps = slice::from_raw_parts((*self.ctx).chapters, num_chapters);
            for chap in chaps {
                let chap = **chap;
                let meta = Dictionary::new(chap.metadata);
                let num = chap.id;
                let title = meta
                    .get("title")
                    .unwrap_or_else(|| format!("Chapter {}", num));
                let start = norm_time(chap.start, chap.time_base);
                let end = norm_time(chap.end, chap.time_base);
                c.push(Chapter {
                    num,
                    title,
                    start,
                    end,
                });
            }
            Some(c)
        }
    }
}

impl Drop for MediaFile {
    fn drop(&mut self) {
        unsafe {
            ffi::avformat_close_input(&mut self.ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_meta() {
        init();
        let mf = MediaFile::open("test_files/test.mp3").unwrap();
        let dur = mf.duration();
        let br = mf.bitrate();
        println!("Duration {}, bitrate {}", dur, br);
        assert!(dur / 1_000 == 283);
        assert!(br == 192);
        assert_eq!("00.uvod", mf.title().unwrap());
        assert_eq!("Stoparuv pruvodce po galaxii", mf.album().unwrap());
        assert_eq!("Vojtěch Dyk", mf.artist().unwrap());
        assert_eq!("Adam Douglas", mf.composer().unwrap());
        assert!(mf.meta("usak").is_none());
        let meta = mf.all_meta();
        assert_eq!("00.uvod", meta.get("title").unwrap());
        unsafe {
            ffi::av_dump_format(mf.ctx, 0, ptr::null(), 0);
        }
    }
}
