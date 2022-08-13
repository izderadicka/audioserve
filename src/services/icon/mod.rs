use anyhow::Result;
use headers::{ContentLength, ContentType};
use hyper::{Body, Response};
use image::io::Reader as ImageReader;
use image::ImageOutputFormat;
use std::{io::Cursor, path::Path};

use crate::util::ResponseBuilderExt;

const ICON_WIDTH: u32 = 128;
const ICON_HEIGHT: u32 = 128;

pub fn icon_response(path: impl AsRef<Path>) -> Result<Response<Body>> {
    let data = scale_cover(path)?;
    Response::builder()
        .status(200)
        .typed_header(ContentLength(data.len() as u64))
        .typed_header(ContentType::png())
        .body(data.into())
        .map_err(anyhow::Error::from)
}

pub fn scale_cover(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let img = ImageReader::open(path)?.decode()?;
    let scaled = img.resize(
        ICON_WIDTH,
        ICON_HEIGHT,
        image::imageops::FilterType::Lanczos3,
    );
    let mut data = Vec::with_capacity(1024);
    let mut buf = Cursor::new(&mut data);
    scaled.write_to(&mut buf, ImageOutputFormat::Png)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_scale_image() -> anyhow::Result<()> {
        let mut data = scale_cover("test_data/cover.jpg")?;
        let mut buf = Cursor::new(&mut data);
        let img2 = ImageReader::with_format(&mut buf, image::ImageFormat::Png).decode()?;
        assert_eq!(ICON_WIDTH, img2.width());
        assert_eq!(ICON_HEIGHT, img2.height());
        assert!(data.len() > 1024);
        Ok(())
    }
}
