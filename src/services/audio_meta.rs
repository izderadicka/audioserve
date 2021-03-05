use super::types::AudioMeta;
use crate::error::{bail, Result};
use std::path::Path;

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
    fn has_chapters(&self) -> bool;
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

        fn has_chapters(&self) -> bool {
            self.media_file.chapters_count() > 1
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
                Some(fname) => match media_info::MediaFile::open(fname) {
                    Ok(media_file) => Ok(Info { media_file }),
                    Err(e) => {
                        error!("Cannot get media info, error {}", e);
                        bail!(e)
                    }
                },
                None => {
                    error!("Invalid file name {:?}, not utf-8", path);
                    bail!("Non UTF-8 file name")
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
