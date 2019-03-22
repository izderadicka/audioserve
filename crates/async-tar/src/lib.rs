extern crate tar;
extern crate tokio;

use futures::future::poll_fn;
use std::ffi::{OsString, OsStr};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs as tokio_fs;
use tokio::prelude::*;
use std::iter::IntoIterator;

const EMPTY_BLOCK: [u8; 512] = [0; 512];
const BUFFER_LENGTH: usize = 8 * 1024; // must be multiple of 512 !!!
const PATH_MAX_LEN: usize = 100; // this is limitation of basic tar header

fn cut_path<P: AsRef<OsStr>>(p:P, max_len: usize) -> OsString {
 let s: OsString = p.as_ref().into();
 if s.len() > max_len {
     let path = Path::new(&s);
     let ext = path.extension().and_then(|e| e.to_str());
     let ext_len = ext.map(|e| e.len()+1).unwrap_or(0);
     let base = path.file_stem().unwrap().to_string_lossy();
     let mut name: String = base.chars().take(max_len-ext_len).collect();
     if ext_len>0 {
        name.push('.');
        name.push_str(ext.unwrap());
     }
     name.into() 
 } else {
     s
 }
}

enum TarState {
    BeforeNext,
    NextFile {
        path: PathBuf,
    },
    OpeningFile {
        file: tokio_fs::file::OpenFuture<PathBuf>,
        fname: OsString,
    },
    PrepareHeader {
        fname: OsString,
        meta: tokio_fs::file::MetadataFuture,
    },
    HeaderReady {
        file: tokio_fs::File,
        fname: OsString,
        meta: fs::Metadata,
    },
    Sending {
        file: tokio_fs::File,
    },

    Finish {
        block: u8,
    },
}

///
/// Calculates size of tar archive from list/iterator of known sizes of it's content.
/// Works only for our case - e.g. contains files only
/// 
pub fn calc_size<S:IntoIterator<Item=u64>>(sizes:S) -> u64 {
    sizes.into_iter().fold(1024, |total,sz| total + 512+ 512*((sz+511)/512))

}

///
/// Tar archive as a Stream
/// Sends chunks of tar archive, which are either tar headers or blocks of data from files
/// 
/// This tar is especially created to send content of directory in HTTP response, 
/// so it does not provide real metadata of files (not to reveal unnecessary details of local implementation).
/// 
/// Only file name is stored in tar (limited to 100 chars), so it's not intended for hiearchical archives.
/// 
pub struct TarStream<P> {
    state: Option<TarState>,
    iter: Box<dyn Iterator<Item = P> + Send>,
    position: usize,
    buf: [u8; BUFFER_LENGTH],
    base_dir: Option<PathBuf>
}

impl TarStream<PathBuf> {
    ///
    /// Create stream that tars all files in given directory
    /// 
    /// Returns furture that resolves to this stream
    /// (as directory listing is done asychronously)
    pub fn tar_dir<P: AsRef<Path> + Send>(
        dir: P,
    ) -> impl Future<Item = Self, Error = io::Error> + Send {
        let dir: PathBuf = dir.as_ref().to_owned();
        let dir = tokio_fs::read_dir(dir).flatten_stream();

        let files = dir
            .and_then(|entry| {
                let path = entry.path();
                poll_fn(move || entry.poll_file_type()).map(|file_type| (path, file_type))
            })
            .filter_map(|(path, file_type)| {
                if file_type.is_file() {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        let ts = files.map(|paths| {
            let iter = paths.into_iter();
            let state = Some(TarState::BeforeNext);
            TarStream {
                state,
                iter: Box::new(iter),
                position: 0,
                buf: [0; BUFFER_LENGTH],
                base_dir: None
            }
        });

        ts
    }
}

impl <P: AsRef<Path> + Send> TarStream<P> {
    ///
    /// Create stream that tars files from given path iterator
    /// 
    pub fn tar_iter<I>(iter:I) -> Self 
    where I: Iterator<Item=P> + Send  + 'static

    {
        TarStream {
            state: Some(TarState::BeforeNext),
            iter: Box::new(iter),
            position: 0,
            buf: [0; BUFFER_LENGTH],
            base_dir: None

        }

    }

    pub fn tar_iter_rel<I, B: AsRef<Path>>(iter:I, base_dir:B) -> Self 
    where I: Iterator<Item=P> + Send  + 'static {
        TarStream {
            state: Some(TarState::BeforeNext),
            iter: Box::new(iter),
            position: 0,
            buf: [0; BUFFER_LENGTH],
            base_dir: Some(base_dir.as_ref().into())

        }
    }
}

impl <P> TarStream<P> {
    fn full_path(&self, rel: PathBuf) -> PathBuf {
        match self.base_dir {
            Some(ref p) => {
                p.clone().join(rel)
            }
            None => rel
        }
    }
}

impl <P: AsRef<Path> + Send> Stream for TarStream<P> {
    type Item = Vec<u8>;
    type Error = io::Error;
    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        loop {
            match self.state.take() {
                None => break,
                Some(state) => {
                    match state {
                        // move to next file
                        TarState::BeforeNext => match self.iter.next() {
                            None => {
                                self.state = Some(TarState::Finish { block: 0 });
                            }
                            Some(path) => {
                                self.state = Some(TarState::NextFile { path: path.as_ref().to_owned() });
                            }
                        },
                        // we start with async opening of file
                        TarState::NextFile { path } => {
                            let fname = path.file_name().map(|name| cut_path(name, PATH_MAX_LEN)).unwrap();
                            let file = tokio_fs::File::open(self.full_path(path));
                            self.state = Some(TarState::OpeningFile { file, fname });
                        }

                        // now test if file is opened
                        TarState::OpeningFile { mut file, fname } => match file.poll() {
                            Ok(Async::NotReady) => {
                                self.state = Some(TarState::OpeningFile { file, fname });
                                return Ok(Async::NotReady);
                            }
                            Ok(Async::Ready(file)) => {
                                let meta = file.metadata();
                                self.state = Some(TarState::PrepareHeader { fname, meta })
                            }

                            Err(e) => return Err(e),
                        },

                        //when file is opened read its metadata
                        TarState::PrepareHeader { fname, mut meta } => match meta.poll() {
                            Ok(Async::NotReady) => {
                                self.state = Some(TarState::PrepareHeader { fname, meta });
                                return Ok(Async::NotReady);
                            }

                            Ok(Async::Ready((file, meta))) => {
                                self.state = Some(TarState::HeaderReady { file, fname, meta });
                            }

                            Err(e) => return Err(e),
                        },

                        // from metadata create tar header
                        TarState::HeaderReady { file, fname, meta } => {
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs();
                            let mut header = tar::Header::new_gnu();
                            header.set_path(fname).expect("cannot set path in header");
                            header.set_size(meta.len());
                            header.set_mode(0o644);
                            header.set_mtime(now);
                            header.set_cksum();
                            let bytes = header.as_bytes();
                            let chunk = bytes.to_vec();
                            self.state = Some(TarState::Sending { file });
                            self.position = 0;
                            return Ok(Async::Ready(Some(chunk)));
                        }

                        // and send file data into stream
                        TarState::Sending { mut file } => {
                            match file.poll_read(&mut self.buf[self.position..]) {
                                Ok(Async::NotReady) => {
                                    self.state = Some(TarState::Sending { file });
                                    return Ok(Async::NotReady);
                                }

                                Ok(Async::Ready(read)) => {
                                    if read == 0 {
                                        self.state = Some(TarState::BeforeNext);
                                        if self.position > 0 {
                                            let rem = self.position % 512;
                                            let padding_length =
                                                if rem > 0 { 512 - rem } else { 0 };
                                            let new_position = self.position + padding_length;
                                            // zeroing padding
                                            &mut self.buf[self.position..new_position]
                                                .copy_from_slice(&mut EMPTY_BLOCK[..padding_length]);
                                            return Ok(Async::Ready(Some(
                                                self.buf[..new_position].to_vec(),
                                            )));
                                        }
                                    } else {
                                        self.position += read;
                                        self.state = Some(TarState::Sending { file });
                                        if self.position == self.buf.len() {
                                            let chunk = self.buf[..self.position].to_vec();
                                            self.position = 0;
                                            return Ok(Async::Ready(Some(chunk)));
                                        }
                                    }
                                }

                                Err(e) => return Err(e),
                            }
                        }
                        // tar format requires two empty blocks at the end
                        TarState::Finish { block } => {
                            if block < 2 {
                                let chunk = EMPTY_BLOCK.to_vec();
                                self.state = Some(TarState::Finish { block: block + 1 });
                                return Ok(Async::Ready(Some(chunk)));
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        }

        Ok(Async::Ready(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::codec::Decoder;

    #[test]
    fn test_tar_from_iter() {
        let temp_dir = tempdir().unwrap();
        let tar_file_name = temp_dir.path().join("test2.tar");
        let tar_file_name2 = tar_file_name.clone();
        let files = &[".gitignore", "Cargo.lock", "Cargo.toml"];
        let sizes = files.iter().map(|f| Path::new(f).metadata().unwrap().len());
        let expected_archive_len = calc_size(sizes);
        let tar_stream = TarStream::tar_iter_rel(files.into_iter(), std::env::current_dir().unwrap());

        {
            let tar_file = tokio_fs::File::create(tar_file_name);
            let f = tar_file.and_then(|f| {
                let codec = tokio::codec::BytesCodec::new();
                let file_sink = codec.framed(f);
                file_sink.send_all(tar_stream.map(|v| v.into()))
            })
            .map(|_r| ())
                .map_err(|e| eprintln!("Error during tar creation: {}", e));

            tokio::run(f);
        }
        let archive_len = tar_file_name2.metadata().unwrap().len();
        assert_eq!(archive_len, expected_archive_len, "archive size is as expected");
        check_archive(tar_file_name2, 3);
        temp_dir.close().unwrap();
    }

    #[test]
    fn test_create_tar() {
        let tar = TarStream::tar_dir(".");
        let temp_dir = tempdir().unwrap();
        let tar_file_name = temp_dir.path().join("test.tar");
        //let tar_file_name = Path::new("/tmp/test.tar");
        let tar_file_name2 = tar_file_name.clone();

        // create tar file asychronously
        {
            let tar_file = tokio_fs::File::create(tar_file_name);
            let f = tar
                .and_then(move |tar_stream| {
                    tar_file.and_then(|f| {
                        let codec = tokio::codec::BytesCodec::new();
                        let file_sink = codec.framed(f);
                        file_sink.send_all(tar_stream.map(|v| v.into()))
                    })
                })
                .map(|_r| ())
                .map_err(|e| eprintln!("Error during tar creation: {}", e));

            tokio::run(f);
        }


        
        check_archive(tar_file_name2, 4);
        temp_dir.close().unwrap();
    }

    fn check_archive(p:PathBuf, num_files: usize) {
        let mut ar = tar::Archive::new(fs::File::open(p).unwrap());

        let entries = ar.entries().unwrap();
        let mut count = 0;
        for entry in entries {
            let mut entry = entry.unwrap();
            let p = entry.path().unwrap().into_owned();

            let mut data_from_archive = vec![];
            let mut data_from_file = vec![];
            entry.read_to_end(&mut data_from_archive).unwrap();
            {
                let mut f = fs::File::open(&p).unwrap();
                f.read_to_end(&mut data_from_file).unwrap();
            }

            println!(
                "File {:?} entry header start {}, file start {}",
                p,
                entry.raw_header_position(),
                entry.raw_file_position()
            );
            println!(
                "File {:?} archive len {}, file len {}",
                p,
                data_from_archive.len(),
                data_from_file.len()
            );

            assert_eq!(
                data_from_archive.len(),
                data_from_file.len(),
                "File len {:?}",
                p
            );

            count+=1;
        }

        assert_eq!(num_files, count, "There are {} files in archive", num_files);
    }

    #[test]
    fn test_cut_path() {
        let a = "abcdef";
        let x = cut_path(a, 10);
        assert_eq!(a, x.to_str().unwrap(), "under limit");

        let a ="0123456789abcd";
        let x = cut_path(a, 10);
        assert_eq!("0123456789", x.to_str().unwrap(), "over limit, no ext");

        let a ="0123456789abcd.mp3";
        let x = cut_path(a, 10);
        assert_eq!("012345.mp3", x.to_str().unwrap(), "over limit, no ext");

    }

}
