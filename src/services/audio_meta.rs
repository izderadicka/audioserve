use super::types::AudioMeta;
use std::path::Path;
use crate::error::Error;

pub struct Chapter {
    pub title:String,
    pub start: u64,
    pub end: u64
}

pub trait MediaInfo<'a>: Sized {
    fn get_audio_info(& self) -> Option<AudioMeta>;
    fn get_chapters(& self) -> Option<Vec<Chapter>>;
    
}

#[cfg(feature = "libavformat")]
mod libavformat {
    use super::*;
    pub struct Info {
        media_file: media_info::MediaFile,
    }
    impl <'a> MediaInfo<'a> for Info {

    fn get_audio_info(&self) -> Option<AudioMeta>{
        Some(AudioMeta {
                    duration: (self.media_file.duration() as f32 / 1000.0).round() as u32,
                    bitrate: self.media_file.bitrate(),
                })
    }
    fn get_chapters(&self) -> Option<Vec<Chapter>>{
        self.media_file.chapters().map(|l| {
            l.into_iter().map(|c| 
            Chapter{
                title: c.title,
                start: c.start,
                end: c.end
            })
            .collect()
        })
    }
    }

    impl Info {

    pub fn from_file(path: &Path) -> Result<Info, Error> {
        match path.as_os_str().to_str() {
        Some(fname) => match media_info::MediaFile::open(fname) {
            Ok(media_file) => {
                Ok(Info{media_file})
            }
            Err(e) => {
                error!("Cannot get media info, error {}", e);
                Err(Error::new_with_cause(e))
            }
        },
        None => {
            error!("Invalid file name {:?}, not utf-8", path);
            Err(Error::new())
        },
    }
    }
    }

    
}

#[cfg(not(feature = "libavformat"))]
mod libtag {
    use super::*;
    pub struct Info<'a> {
        media_file: taglib::File,
        path: &'a Path
    }
    impl <'a> MediaInfo<'a> for Info<'a> {
        fn get_audio_info(&self) -> Option<AudioMeta> {
            match self.media_file.audioproperties() {
                            Ok(ap) => {
                                Some(AudioMeta {
                                    duration: ap.length(),
                                    bitrate: {
                                        let mut bitrate = ap.bitrate();
                                        let duration = ap.length();
                                        if bitrate == 0 && duration != 0 {
                                            // estimate from duration and file size
                                            // Will not work well for small files
                                            if let Ok(size) =
                                                self.path.metadata().map(|m| m.len())
                                            {
                                                bitrate =
                                                    (size * 8 / u64::from(duration) / 1024) as u32;
                                                debug!("Estimating bitrate to {}", bitrate);
                                            };
                                        }
                                        bitrate
                                    },
                                })
                            }
                            Err(e) => {
                                error!("File {:?} does not have audioproperties {:?}", self.path, e);
                                None 
                            }
                        }
            
        }
        fn get_chapters(&self) -> Option<Vec<Chapter>> {
            None
        }
    }

impl <'a> Info<'a> {

        pub fn from_file(path: &'a Path) -> Result<Info<'a>, Error> {
           let filename = path.as_os_str().to_str();
            match filename {
                Some(fname) => {
                    let audio_file = taglib::File::new(fname);
                    match audio_file {
                        Ok(media_file) => Ok(Info{media_file, path}),
                        Err(e) => {
                            error!("Cannot get audiofile {} error {:?}", fname, e);
                            Err(Error::new())
                        }
                    }
                }
                None => {
                    error!("File name {:?} is not utf8", filename);
                    Err(Error::new())
                }
            }
        }
    }
}

#[cfg(feature = "libavformat")]
pub fn get_audio_properties<'a>(audio_file_name: &'a Path) -> Result<impl MediaInfo<'a>, Error> {
    libavformat::Info::from_file(audio_file_name)
}

#[cfg(not(feature = "libavformat"))]
pub fn get_audio_properties<'a>(audio_file_name: &'a Path) -> Result<impl MediaInfo<'a>, Error> {
    libtag::Info::from_file(audio_file_name)
}
