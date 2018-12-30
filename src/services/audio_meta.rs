use std::path::Path;
use super::types::AudioMeta;

#[cfg(feature="libavformat")]
pub fn get_audio_properties(audio_file_name: &Path) -> Option<AudioMeta> {
    match audio_file_name.as_os_str().to_str() {
        Some(fname) => match media_info::MediaFile::open(fname) {
            Ok(media_file) => {
                return Some(AudioMeta{
                    duration: (media_file.duration() / 1000 ) as u32,
                    bitrate: media_file.bitrate()
                })
            },
            Err(e) => error!("Cannot get media info, error {}", e)

            }   
        None => error!("Invalid file name {:?}, not utf-8", audio_file_name)
    }
    

    None
}

#[cfg(not(feature="libavformat"))]
pub fn get_audio_properties(audio_file_name: &Path) -> Option<AudioMeta> {
    let filename = audio_file_name.as_os_str().to_str();
    match filename {
        Some(fname) => {
            let audio_file = taglib::File::new(fname);
            match audio_file {
                Ok(f) => match f.audioproperties() {
                    Ok(ap) => {
                        return Some(AudioMeta {
                            duration: ap.length(),
                            bitrate: {
                                let mut bitrate = ap.bitrate();
                                let duration = ap.length();
                                if bitrate == 0 && duration != 0 {
                                    // estimate from duration and file size
                                    // Will not work well for small files
                                    if let Ok(size) = audio_file_name.metadata().map(|m| m.len()) {
                                        bitrate = (size * 8 / u64::from(duration) / 1024) as u32;
                                        debug!("Estimating bitrate to {}", bitrate);
                                    };
                                }
                                bitrate
                            },
                        });
                    }
                    Err(e) => warn!("File {} does not have audioproperties {:?}", fname, e),
                },
                Err(e) => warn!("Cannot get audiofile {} error {:?}", fname, e),
            }
        }
        None => warn!("File name {:?} is not utf8", filename),
    };

    None
}