use anyhow::Result;
use collection::{audio_meta::is_audio, extract_cover};
use image::io::Reader as ImageReader;
use image::ImageFormat;
use simple_file_cache::FileModTime;
use std::{
    io::{Cursor, Read},
    path::Path,
};

use self::cache::{cache_icon, cached_icon};
use crate::config::get_config;
use myhy::response::{data_response, HttpResponse};

pub mod cache;

pub fn icon_response(
    path: impl AsRef<Path> + std::fmt::Debug,
    mtime: FileModTime,
) -> Result<HttpResponse> {
    let cache_enabled = !get_config().icons.cache_disabled;
    let data = match if cache_enabled {
        cached_icon(&path, mtime)
    } else {
        None
    } {
        Some(mut f) => {
            let mut data = Vec::with_capacity(1024);
            f.read_to_end(&mut data)?;
            data
        }
        None => {
            let data = scale_cover(&path)?;
            if cache_enabled {
                cache_icon(path, &data, mtime)
                    .unwrap_or_else(|e| error!("error adding icon to cache: {}", e));
            }
            data
        }
    };

    Ok(data_response(
        data,
        mime::IMAGE_PNG,
        get_config().folder_file_cache_age,
        None,
        false,
    ))
}

pub fn scale_cover(path: impl AsRef<Path> + std::fmt::Debug) -> Result<Vec<u8>> {
    use image::imageops::FilterType;
    let img = if is_audio(&path) {
        let data = extract_cover(&path)
            .ok_or_else(|| anyhow::Error::msg("Cover is missing, but is expected"))?;
        ImageReader::new(Cursor::new(data))
            .with_guessed_format()?
            .decode()?
    } else {
        ImageReader::open(&path)?.with_guessed_format()?.decode()?
    };
    let sz = get_config().icons.size;
    let scaled = img.resize(
        sz,
        sz,
        if !get_config().icons.fast_scaling {
            FilterType::Lanczos3
        } else {
            FilterType::Triangle
        },
    );
    let mut data = Vec::with_capacity(1024);
    let mut buf = Cursor::new(&mut data);
    scaled.write_to(&mut buf, ImageFormat::Png)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::init::init_default_config;
    #[test]
    fn test_scale_image() -> anyhow::Result<()> {
        init_default_config();
        let mut data = scale_cover("test_data/cover.jpg")?;
        let mut buf = Cursor::new(&mut data);
        let img2 = ImageReader::with_format(&mut buf, image::ImageFormat::Png).decode()?;
        let sz = get_config().icons.size;
        assert_eq!(sz, img2.width());
        assert_eq!(sz, img2.height());
        assert!(data.len() > 1024);
        Ok(())
    }
}
