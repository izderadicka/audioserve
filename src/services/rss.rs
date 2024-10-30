use std::{fs, path::Path};

use crate::{config::get_config, Error};
use anyhow::bail;
use chrono::DateTime;
use collection::audio_meta::AudioFolder;
use rss::{Channel, ChannelBuilder, EnclosureBuilder, ImageBuilder, ItemBuilder};

pub fn folder_to_channel(
    base_path: &Path,
    collection: usize,
    path: &Path,
    folder: AudioFolder,
) -> Result<Channel, Error> {
    let title = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Not folder path"))?
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid path - cannot be converted to UTF8"))?;
    let mut path = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid path - cannot be converted to UTF8"))?;

    if path.starts_with('/') {
        path = &path[1..];
    }

    let base_url = get_config()
        .url_base
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No base URL is defined"))?;
    let alt_base_url = get_config()
        .url_path_prefix
        .as_ref()
        .map(|p| base_url.join(&p))
        .transpose()?;
    let base_url = match alt_base_url {
        Some(ref url) => url,
        None => base_url,
    };
    let base_url = base_url.join(&format!("{}/", collection))?;

    let link = base_url.join("feed/")?.join(path)?;
    let mut cb = ChannelBuilder::default();
    let channel = cb.title(title).link(link);

    if let Some(cover) = folder.cover {
        let cover_path = cover
            .path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid cover path"))?;
        let image_url = base_url.join("cover/")?.join(cover_path)?;
        let image = ImageBuilder::default()
            .url(image_url)
            .title("cover")
            .build();
        channel.image(image);
    }

    if let Some(description) = folder.description {
        let description_path = base_path.join(description.path);
        let description = read_file_with_limit(&description_path)?;
        channel.description(description);
    }

    let mut channel = channel.build();
    let mut files = folder.files;
    files.sort_unstable_by(|a, b| b.modified.cmp(&a.modified));
    let items = files
        .into_iter()
        .map(|f| {
            let file_path = f
                .path
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid file path - not UTF8"))?;
            let file_link = base_url.join("audio/")?.join(file_path)?;
            let enclosure = EnclosureBuilder::default()
                .url(file_link)
                .length(
                    f.size
                        .map(|size| size.to_string())
                        .unwrap_or_else(|| "0".into()),
                )
                .mime_type(f.mime)
                .build();
            let publication_date = f
                .modified
                .and_then(|t| DateTime::from_timestamp_millis(t.timestamp_millis() as i64))
                .map(|dt| dt.to_rfc2822());
            Ok(ItemBuilder::default()
                .title(Some(f.name.into()))
                .pub_date(publication_date)
                .link(None)
                .enclosure(Some(enclosure))
                .build())
        })
        .collect::<Result<Vec<_>, anyhow::Error>>()?;
    channel.set_items(items);
    Ok(channel)
}

fn read_file_with_limit(path: &Path) -> Result<String, anyhow::Error> {
    if path.metadata()?.len() > 64 * 1024 * 1024 {
        bail!("Description file too big");
    }
    let s = fs::read_to_string(path)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;

    #[test]
    fn test_url_joining() -> Result<(), Error> {
        let base: Url = "http://localhost:3000".parse()?;
        let url = base.join("audio/")?.join("cesta/z/mesta")?;
        assert_eq!("http://localhost:3000/audio/cesta/z/mesta", url.to_string());
        Ok(())
    }
}
