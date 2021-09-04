use serde_derive::Serialize;
use crate::error::{Error, Result};
use std::path::Path;

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct AudioMeta {
    pub duration: u32, // duration in seconds, if available
    pub bitrate: u32,  // bitrate in kB/s
}

#[derive(Clone, Copy, Debug)]
pub struct TimeSpan {
    pub start: u64,
    pub duration: Option<u64>,
}

impl std::fmt::Display for TimeSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        match self.duration {
            Some(d) => write!(f, "{}-{}", self.start, d),
            None => write!(f, "{}", self.start),
        }
    }
}

pub struct Chapter {
    pub number: u32,
    pub title: String,
    pub start: u64,
    pub end: u64,
}

/// trait to generalize access to media metadata
/// (so that underlying library can be easily changed)
pub trait MediaInfo<'a>: Sized {
    fn get_audio_info(&self) -> Option<AudioMeta>;
    fn get_chapters(&self) -> Option<Vec<Chapter>>;
}

mod libavformat {
    use super::*;
    use std::sync::Once;

    static INIT_LIBAV: Once = Once::new();

    pub fn init() {
        INIT_LIBAV.call_once(media_info::init)
    }

    pub struct Info {
        media_file: media_info::MediaFile,
    }
    impl<'a> MediaInfo<'a> for Info {
        fn get_audio_info(&self) -> Option<AudioMeta> {
            Some(AudioMeta {
                duration: (self.media_file.duration() as f32 / 1000.0).round() as u32,
                bitrate: self.media_file.bitrate(),
            })
        }
        fn get_chapters(&self) -> Option<Vec<Chapter>> {
            self.media_file.chapters().map(|l| {
                l.into_iter()
                    .map(|c| Chapter {
                        number: c.num as u32,
                        title: c.title,
                        start: c.start,
                        end: c.end,
                    })
                    .collect()
            })
        }
    }

    impl Info {
        pub fn from_file(path: &Path) -> Result<Info> {
            match path.as_os_str().to_str() {
                Some(fname) => Ok(Info { media_file:  media_info::MediaFile::open(fname)?}),
                None => {
                    error!("Invalid file name {:?}, not utf-8", path);
                    Err(Error::InvalidFileName)
                }
            }
        }
    }
}

pub fn get_audio_properties(audio_file_name: &Path) -> Result<impl MediaInfo> {
    libavformat::Info::from_file(audio_file_name)
}

pub fn init_media_lib() {
    libavformat::init()
}
