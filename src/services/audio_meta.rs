use super::types::AudioMeta;
use std::path::Path;
use crate::error::Error;

pub struct Chapter {
    pub number: u32,
    pub title:String,
    pub start: u64,
    pub end: u64
}

pub trait MediaInfo<'a>: Sized {
    fn get_audio_info(& self) -> Option<AudioMeta>;
    fn get_chapters(& self) -> Option<Vec<Chapter>>;
    
}

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
                number: c.num as u32,
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
            Err(Error::new_with_cause("Non UTF-8 file name"))
        },
    }
    }
    }

    
}


pub fn get_audio_properties<'a>(audio_file_name: &'a Path) -> Result<impl MediaInfo<'a>, Error> {
    libavformat::Info::from_file(audio_file_name)
}
