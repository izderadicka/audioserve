#[cfg(feature = "alternate-encoding")]
use encoding::{label::encoding_from_whatwg_label, DecoderTrap, EncodingRef};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::marker::PhantomData;
use std::os::raw::c_char;
use std::ptr;
use std::slice;
use thiserror::Error;

#[allow(dead_code)]
#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(deref_nullptr)]
#[allow(clippy::type_complexity)]
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

const CODEC_ID_MJPEG: u32 = 7;

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

fn norm_time(t: i64, time_base: ffi::AVRational) -> u64 {
    assert!(t >= 0);
    t as u64 * 1000 * time_base.num as u64 / time_base.den as u64
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
    pub fn new(dic: *mut ffi::AVDictionary) -> Self {
        #[cfg(feature = "alternate-encoding")]
        return Dictionary {
            dic,
            alternate_encoding: None,
        };
        #[cfg(not(feature = "alternate-encoding"))]
        return Dictionary { dic };
    }

    pub fn len(&self) -> usize {
        if self.dic.is_null() {
            return 0;
        }
        unsafe { ffi::av_dict_count(self.dic) as usize }
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

    pub fn has_key(&self, key: impl AsRef<str>) -> bool {
        let res = self.get_entry(key);
        !res.is_null()
    }

    fn get_entry(&self, key: impl AsRef<str>) -> *const ffi::AVDictionaryEntry {
        if self.dic.is_null() {
            return ptr::null();
        }
        let cs = CString::new(key.as_ref()).expect("zero byte in key");
        unsafe { ffi::av_dict_get(self.dic, cs.as_ptr(), ptr::null(), 0) }
    }

    pub fn get<S: AsRef<str>>(&self, key: S) -> Option<String> {
        let res = self.get_entry(key);
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

    pub fn get_all(&self) -> HashMap<String, String> {
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
        //ffi::av_register_all()
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
        let mut ctx;
        let meta;
        unsafe {
            ctx = ffi::avformat_alloc_context();
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

            meta = Dictionary::new((*ctx).metadata);
        }

        let mut mf = MediaFile { ctx, meta };

        if mf.meta.len() == 0 && mf.streams_count() > 0 {
            //OK we do not have meta in main header, let's look at streams
            for idx in 0..mf.streams_count() {
                let s = mf.stream(idx);
                if matches!(s.kind(), StreamKind::AUDIO) {
                    mf.meta = s.meta();
                    break;
                }
            }
        }
        Ok(mf)
    }

    pub fn streams_count(&self) -> usize {
        unsafe { (*self.ctx).nb_streams.try_into().unwrap_or_default() }
    }

    pub fn stream(&self, idx: usize) -> Stream {
        let streams = unsafe { slice::from_raw_parts((*self.ctx).streams, self.streams_count()) };
        Stream {
            ctx: streams[idx],
            _parent: PhantomData,
        }
    }

    fn attached_stream(&self) -> Option<Stream> {
        for idx in 0..self.streams_count() {
            let s = self.stream(idx);
            if matches!(s.kind(), StreamKind::VIDEO) && s.codec_id() == CODEC_ID_MJPEG {
                let pic = s.picture();
                if let Some(p) = pic {
                    if p.len() > 100 {
                        return Some(s);
                    }
                }
            }
        }
        None
    }

    pub fn cover(&self) -> Option<Vec<u8>> {
        self.attached_stream()
            .and_then(|s| s.picture().map(|s| s.to_vec()))
    }

    pub fn has_cover(&self) -> bool {
        self.attached_stream().is_some()
    }

    pub fn has_meta(&self, key: impl AsRef<str>) -> bool {
        self.meta.has_key(key)
    }

    #[cfg(feature = "alternate-encoding")]
    pub fn open_with_encoding<S: AsRef<str>>(
        fname: S,
        alternate_encoding: Option<impl AsRef<str>>,
    ) -> Result<Self> {
        MediaFile::open(fname).and_then(|mut mf| match alternate_encoding {
            Some(e) => {
                let new_dict = Dictionary::new_with_encoding(mf.meta.dic, e)?;
                mf.meta = new_dict;
                Ok(mf)
            }
            None => Ok(mf),
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
        unsafe {
            let num_chapters = (*self.ctx).nb_chapters as usize;
            if num_chapters == 0 {
                return None;
            }
            let mut c = Vec::new();
            let chaps = slice::from_raw_parts((*self.ctx).chapters, num_chapters);
            for chap in chaps {
                let chap = **chap;
                // TODO: May need alternate encoding also for chapter names
                let meta = Dictionary::new(chap.metadata);
                let num = chap.id;
                let title = meta
                    .get("title")
                    .unwrap_or_else(|| format!("Chapter {}", num));
                let start = norm_time(chap.start, chap.time_base);
                let end = norm_time(chap.end, chap.time_base);
                c.push(Chapter {
                    num: num.try_into().unwrap_or(std::i32::MAX),
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

pub struct Stream<'a> {
    ctx: *mut ffi::AVStream,
    _parent: PhantomData<&'a MediaFile>,
}

impl<'a> Stream<'a> {
    pub fn kind(&self) -> StreamKind {
        let codec_type = unsafe { (*(*self.ctx).codecpar).codec_type };
        use StreamKind::*;
        match codec_type {
            ffi::AVMediaType_AVMEDIA_TYPE_VIDEO => VIDEO,
            ffi::AVMediaType_AVMEDIA_TYPE_AUDIO => AUDIO,
            ffi::AVMediaType_AVMEDIA_TYPE_DATA => DATA,
            ffi::AVMediaType_AVMEDIA_TYPE_SUBTITLE => SUBTITLE,
            ffi::AVMediaType_AVMEDIA_TYPE_ATTACHMENT => ATTACHMENT,
            _ => UNKNOWN,
        }
    }

    fn meta(&self) -> Dictionary {
        Dictionary::new(unsafe { (*self.ctx).metadata })
    }

    pub fn duration(&self) -> u64 {
        norm_time(
            unsafe { *self.ctx }.duration,
            unsafe { *self.ctx }.time_base,
        )
    }

    pub fn frames_count(&self) -> u64 {
        unsafe { *self.ctx }
            .nb_frames
            .try_into()
            .unwrap_or_default()
    }

    pub fn id(&self) -> i32 {
        unsafe { *self.ctx }.id
    }

    pub fn codec_id(&self) -> u32 {
        unsafe { *(*self.ctx).codecpar }.codec_id
    }

    pub fn codec_four_cc(&self) -> String {
        let n = unsafe { *(*self.ctx).codecpar }.codec_tag;
        let bytes = n.to_le_bytes();
        std::str::from_utf8(&bytes)
            .map(|s| s.to_string())
            .unwrap_or_else(|_| format!("#{:X}", n))
    }

    pub fn codec_four_cc_raw(&self) -> u32 {
        unsafe { *(*self.ctx).codecpar }.codec_tag
    }

    pub fn bitrate(&self) -> u32 {
        (unsafe { *(*self.ctx).codecpar }.bit_rate / 1000) as u32
    }

    pub fn disposition(&self) -> i32 {
        unsafe { *self.ctx }.disposition
    }

    pub fn picture(&self) -> Option<&[u8]> {
        let pic: ffi::AVPacket = unsafe { *self.ctx }.attached_pic;
        if pic.size <= 0 {
            None
        } else {
            // may not panic as int is positive
            Some(unsafe { slice::from_raw_parts(pic.data, pic.size.try_into().unwrap()) })
        }
    }
}

#[derive(Debug)]
pub enum StreamKind {
    AUDIO,
    VIDEO,
    DATA,
    SUBTITLE,
    ATTACHMENT,
    UNKNOWN,
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
