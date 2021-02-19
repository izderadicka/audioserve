use std::{borrow::Cow, path::Path};

use bytes::{BufMut, BytesMut};

use crate::error::Result;
use crate::{date::Timestamp, error::Error};

const DIRECTORY_END_SIZE: u32 = 22;
const FILE_HEADER_SIZE: u32 = 30;
const DATA_DESCRIPTOR_SIZE: u32 = 16;
const DIRECTORY_ENTRY_SIZE: u32 = 46;

const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x04034b50;
const CENTRAL_DIRECTORY_HEADER_SIGNATURE: u32 = 0x02014b50;
const CENTRAL_DIRECTORY_END_SIGNATURE: u32 = 0x06054b50;
const DATA_DESCRIPTOR_SIGNATURE: u32 = 0x08074b;

const MIN_VERSION: u16 = 20;
const FLAGS: u16 = 0b0000_1000_0000_1000;
const COMPRESS_STORE: u16 = 0;

pub fn calc_size<P, I>(sizes: I) -> Result<u64>
where
    I: IntoIterator<Item = (P, u64)>,
    P: AsRef<Path>,
{
    // let mut size: u64 = DIRECTORY_END_SIZE as u64;
    // for (path, sz) in sizes.into_iter() {
    //     size += (path_to_file_name(&path))?.len() as u64 + FILE_HEADER_SIZE as u64 + sz + DATA_DESCRIPTOR_SIZE as u64;
    // }
    // Ok(size)
    sizes
        .into_iter()
        .try_fold(DIRECTORY_END_SIZE as u64, |total, (path, sz)| {
            Ok(total
                + FILE_HEADER_SIZE as u64
                + 2 * path_to_file_name(&path)?.len() as u64
                + sz
                + DATA_DESCRIPTOR_SIZE as u64
                + DIRECTORY_ENTRY_SIZE as u64)
        })
}

fn path_to_file_name<P: AsRef<Path>>(path: &P) -> Result<Cow<'_, str>> {
    Ok(path
        .as_ref()
        .file_name()
        .ok_or(Error::InvalidPath)?
        .to_string_lossy())
}
pub trait ToBytes {
    fn to_bytes(&self) -> Result<Vec<u8>>;
}

pub struct FileHeader {
    file_name: String,
    modified: Timestamp,
}

impl FileHeader {
    pub fn new(path: impl AsRef<Path>, modified: impl Into<Timestamp>) -> Result<Self> {
        let file_name = path_to_file_name(&path)?.to_string();
        Ok(FileHeader {
            file_name,
            modified: modified.into(),
        })
    }
}

impl ToBytes for FileHeader {
    fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut h = BytesMut::with_capacity(FILE_HEADER_SIZE as usize + self.file_name.len());

        // local file header signature
        h.put_u32_le(LOCAL_FILE_HEADER_SIGNATURE);
        // version needed to extract
        h.put_u16_le(MIN_VERSION);
        // general purpose bit flag
        h.put_u16_le(FLAGS);
        // Compression method
        h.put_u16_le(COMPRESS_STORE);
        // last mod file time and last mod file date
        h.put_u16_le(self.modified.dos_timepart());
        h.put_u16_le(self.modified.dos_datepart()?);
        // crc-32
        h.put_u32_le(0);
        // compressed size
        h.put_u32_le(0);
        // uncompressed size
        h.put_u32_le(0);
        // file name length
        if self.file_name.len() > std::u16::MAX as usize {
            return Err(Error::FileNameTooBig);
        }
        h.put_u16_le(self.file_name.as_bytes().len() as u16);
        // extra field length
        h.put_u16_le(0);
        // file name
        h.put_slice(self.file_name.as_bytes());

        Ok(h.to_vec())
    }
}

pub struct Descriptor {
    size: u64,
    crc: u32,
}

impl Descriptor {
    pub fn new(size: u64, crc: u32) -> Self {
        Descriptor { size, crc }
    }
}

impl ToBytes for Descriptor {
    fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut d = BytesMut::with_capacity(DATA_DESCRIPTOR_SIZE as usize);

        if self.size > std::u32::MAX as u64 {
            return Err(Error::FileTooBig(self.size));
        }

        // data_descriptor header signature
        d.put_u32_le(DATA_DESCRIPTOR_SIGNATURE);
        // crc-32
        d.put_u32_le(self.crc);
        // compressed size
        d.put_u32_le(self.size as u32);
        // uncompressed size
        d.put_u32_le(self.size as u32);

        Ok(d.to_vec())
    }
}

pub struct DirectoryEntry {
    header: FileHeader,
    desc: Descriptor,
    offset: u64,
}

impl DirectoryEntry {
    fn size(&self) -> u32 {
        (self.header.file_name.len() + DIRECTORY_END_SIZE as usize) as u32
    }
}

impl DirectoryEntry {
    fn add_to_bytes<T: BufMut>(&self, buf: &mut T) -> Result<()> {
        // central file header signature
        buf.put_u32_le(CENTRAL_DIRECTORY_HEADER_SIGNATURE);
        // version made by
        buf.put_u16_le(MIN_VERSION);
        // version needed to extract
        buf.put_u16_le(MIN_VERSION);
        // general puprose bit flag
        buf.put_u16_le(FLAGS);
        // compression method
        buf.put_u16_le(COMPRESS_STORE);
        // last mod file time + date
        buf.put_u16_le(self.header.modified.dos_timepart());
        buf.put_u16_le(self.header.modified.dos_datepart()?);
        // crc-32
        buf.put_u32_le(self.desc.crc);
        // compressed size
        if self.desc.size > std::u32::MAX as u64 {
            return Err(Error::FileTooBig(self.desc.size));
        }
        buf.put_u32_le(self.desc.size as u32);
        // uncompressed size
        buf.put_u32_le(self.desc.size as u32);
        // file name length
        if self.header.file_name.len() > std::u16::MAX as usize {
            return Err(Error::FileNameTooBig);
        }
        buf.put_u16_le(self.header.file_name.as_bytes().len() as u16);
        // extra field length
        buf.put_u16_le(0);
        // file comment length
        buf.put_u16_le(0);
        // disk number start
        buf.put_u16_le(0);
        // internal file attributes
        buf.put_u16_le(0);
        // external file attributes
        buf.put_u32_le(0);
        // relative offset of local header
        if self.offset > std::u32::MAX as u64 {
            return Err(Error::ArchiveTooBig);
        }
        buf.put_u32_le(self.offset as u32);
        // file name
        buf.put_slice(self.header.file_name.as_bytes());
        // extra field
        // file comment
        // <none>

        Ok(())
    }
}

struct DirectoryEnd {
    number_of_files: u16,
    dir_size: u32,
    dir_offset: u64,
}

impl DirectoryEnd {
    fn add_to_bytes<T: BufMut>(&self, buf: &mut T) -> Result<()> {
        // signature
        buf.put_u32_le(CENTRAL_DIRECTORY_END_SIGNATURE);
        // disk number
        buf.put_u16_le(0);
        // disk with central directory
        buf.put_u16_le(0);
        //number of files on this disk
        buf.put_u16_le(self.number_of_files);
        // total number of files
        buf.put_u16_le(self.number_of_files);
        // directory size
        buf.put_u32_le(self.dir_size);
        // directory offset from start
        if self.dir_offset > std::u32::MAX as u64 {
            return Err(Error::ArchiveTooBig);
        }
        buf.put_u32_le(self.dir_offset as u32);
        // Comment length
        buf.put_u16_le(0);
        // Comment
        //buf.put_all(&self.zip_file_comment);

        Ok(())
    }
}

pub struct Directory {
    entries: Vec<DirectoryEntry>,
    offset: Option<u64>,
}

impl Directory {
    pub fn new() -> Self {
        Directory {
            entries: Vec::new(),
            offset: None,
        }
    }

    pub fn add_entry(&mut self, header: FileHeader, desc: Descriptor, offset: u64) {
        self.entries.push(DirectoryEntry {
            header,
            desc,
            offset,
        })
    }

    pub fn finalize(mut self, offset: u64) -> Result<Vec<u8>> {
        self.offset = Some(offset);
        self.to_bytes()
    }
}

impl ToBytes for Directory {
    fn to_bytes(&self) -> Result<Vec<u8>> {
        let num_files = self.entries.len();
        let cap = self.entries.iter().map(|e| e.size()).sum::<u32>() + DIRECTORY_END_SIZE;
        let mut d = BytesMut::with_capacity(cap as usize);
        for e in &self.entries {
            e.add_to_bytes(&mut d)?;
        }

        let dir_size = d.len();

        let offset = self
            .offset
            .expect("invalid state - must update offset first");
        if offset > std::u32::MAX as u64 {
            return Err(Error::ArchiveTooBig);
        }
        let end = DirectoryEnd {
            dir_offset: offset,
            dir_size: dir_size as u32,
            number_of_files: num_files as u16,
        };

        end.add_to_bytes(&mut d)?;

        Ok(d.to_vec())
    }
}
