extern crate tar;
extern crate tokio;

use futures::{future::Future, stream::Stream};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::iter::IntoIterator;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::{fs as tokio_fs, io::AsyncRead};

const EMPTY_BLOCK: [u8; 512] = [0; 512];
const BUFFER_LENGTH: usize = 8 * 1024; // must be multiple of 512 !!!
const PATH_MAX_LEN: usize = 100; // this is limitation of basic tar header

fn cut_path<P: AsRef<OsStr>>(p: P, max_len: usize) -> OsString {
    let s: OsString = p.as_ref().into();
    if s.len() > max_len {
        let path = Path::new(&s);
        let ext = path.extension().and_then(OsStr::to_str);
        let ext_len = ext.map(|e| e.len() + 1).unwrap_or(0);
        let base = path.file_stem().unwrap().to_string_lossy();
        let mut name: String = base.chars().take(max_len - ext_len).collect();
        if ext_len > 0 {
            name.push('.');
            name.push_str(ext.unwrap());
        }
        name.into()
    } else {
        s
    }
}

type PinnedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + Sync>>;

#[allow(clippy::large_enum_variant)] // not a problem as there is only one instance of state
enum TarState {
    BeforeNext,
    NextFile {
        path: PathBuf,
    },
    OpeningFile {
        file: PinnedFuture<Result<tokio_fs::File, io::Error>>,
        fname: OsString,
    },
    PrepareHeader {
        fname: OsString,
        meta: PinnedFuture<(Result<fs::Metadata, io::Error>, tokio_fs::File)>,
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
pub fn calc_size<S: IntoIterator<Item = u64>>(sizes: S) -> u64 {
    sizes
        .into_iter()
        .fold(1024, |total, sz| total + 512 + 512 * ((sz + 511) / 512))
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
    iter: Box<dyn Iterator<Item = P> + Send + Sync>,
    position: usize,
    buf: [u8; BUFFER_LENGTH],
    base_dir: Option<PathBuf>,
}

impl TarStream<PathBuf> {
    ///
    /// Create stream that tars all files in given directory
    ///
    /// Returns furture that resolves to this stream
    /// (as directory listing is done asychronously)
    pub async fn tar_dir<P: AsRef<Path> + Send>(dir: P) -> Result<Self, io::Error> {
        let dir: PathBuf = dir.as_ref().to_owned();
        let mut dir = tokio_fs::read_dir(dir).await?;
        let mut files = Vec::new();
        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            let res = entry.file_type().await;
            if let Ok(file_type) = res {
                if file_type.is_file() {
                    files.push(path)
                }
            };
        }

        let iter = files.into_iter();
        let state = Some(TarState::BeforeNext);
        Ok(TarStream {
            state,
            iter: Box::new(iter),
            position: 0,
            buf: [0; BUFFER_LENGTH],
            base_dir: None,
        })
    }
}

impl<P: AsRef<Path> + Send> TarStream<P> {
    ///
    /// Create stream that tars files from given path iterator
    ///
    pub fn tar_iter<I>(iter: I) -> Self
    where
        I: Iterator<Item = P> + Send + Sync + 'static,
    {
        TarStream {
            state: Some(TarState::BeforeNext),
            iter: Box::new(iter),
            position: 0,
            buf: [0; BUFFER_LENGTH],
            base_dir: None,
        }
    }

    pub fn tar_iter_rel<I, B: AsRef<Path>>(iter: I, base_dir: B) -> Self
    where
        I: Iterator<Item = P> + Send + Sync + 'static,
    {
        TarStream {
            state: Some(TarState::BeforeNext),
            iter: Box::new(iter),
            position: 0,
            buf: [0; BUFFER_LENGTH],
            base_dir: Some(base_dir.as_ref().into()),
        }
    }
}

impl<P> TarStream<P> {
    fn full_path(&self, rel: PathBuf) -> PathBuf {
        match self.base_dir {
            Some(ref p) => p.clone().join(rel),
            None => rel,
        }
    }
}

impl<P: AsRef<Path> + Send> Stream for TarStream<P> {
    type Item = Result<Vec<u8>, io::Error>;
    fn poll_next(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
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
                                self.state = Some(TarState::NextFile {
                                    path: path.as_ref().to_owned(),
                                });
                            }
                        },
                        // we start with async opening of file
                        TarState::NextFile { path } => {
                            let fname = path
                                .file_name()
                                .map(|name| cut_path(name, PATH_MAX_LEN))
                                .unwrap();
                            let file = tokio_fs::File::open(self.full_path(path));
                            self.state = Some(TarState::OpeningFile {
                                file: Box::pin(file),
                                fname,
                            });
                        }

                        // now test if file is opened
                        TarState::OpeningFile {
                            file: mut file_fut,
                            fname,
                        } => match file_fut.as_mut().poll(ctx) {
                            Poll::Pending => {
                                self.state = Some(TarState::OpeningFile {
                                    file: file_fut,
                                    fname,
                                });
                                return Poll::Pending;
                            }
                            Poll::Ready(Ok(file)) => {
                                let meta = async move {
                                    let meta = file.metadata().await;
                                    (meta, file)
                                };
                                self.state = Some(TarState::PrepareHeader {
                                    fname,
                                    meta: Box::pin(meta),
                                });
                            }

                            Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                        },

                        //when file is opened read its metadata
                        TarState::PrepareHeader { fname, mut meta } => {
                            match meta.as_mut().poll(ctx) {
                                Poll::Pending => {
                                    self.state = Some(TarState::PrepareHeader { fname, meta });
                                    return Poll::Pending;
                                }

                                Poll::Ready((Ok(meta), file)) => {
                                    self.state = Some(TarState::HeaderReady { file, fname, meta });
                                }

                                Poll::Ready((Err(e), _)) => return Poll::Ready(Some(Err(e))),
                            }
                        }

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
                            return Poll::Ready(Some(Ok(chunk)));
                        }

                        //and send file data into stream
                        TarState::Sending { mut file } => {
                            let pos = self.position;
                            let mut buf = tokio::io::ReadBuf::new(&mut self.buf[pos..]);
                            match Pin::new(&mut file).poll_read(ctx, &mut buf) {
                                Poll::Pending => {
                                    self.state = Some(TarState::Sending { file });
                                    return Poll::Pending;
                                }

                                Poll::Ready(Ok(_)) => {
                                    let read = buf.filled().len();
                                    if read == 0 {
                                        self.state = Some(TarState::BeforeNext);
                                        if pos > 0 {
                                            let rem = pos % 512;
                                            let padding_length =
                                                if rem > 0 { 512 - rem } else { 0 };
                                            let new_position = pos + padding_length;
                                            // zeroing padding
                                            self.buf[pos..new_position]
                                                .copy_from_slice(&EMPTY_BLOCK[..padding_length]);
                                            return Poll::Ready(Some(Ok(
                                                self.buf[..new_position].to_vec()
                                            )));
                                        }
                                    } else {
                                        self.position += read;
                                        self.state = Some(TarState::Sending { file });
                                        if self.position == self.buf.len() {
                                            let chunk = self.buf[..self.position].to_vec();
                                            self.position = 0;
                                            return Poll::Ready(Some(Ok(chunk)));
                                        }
                                    }
                                }

                                Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                            }
                        }

                        // tar format requires two empty blocks at the end
                        TarState::Finish { block } => {
                            if block < 2 {
                                let chunk = EMPTY_BLOCK.to_vec();
                                self.state = Some(TarState::Finish { block: block + 1 });
                                return Poll::Ready(Some(Ok(chunk)));
                            } else {
                                break;
                            }
                        } //_ => unimplemented!()
                    }
                }
            }
        }

        Poll::Ready(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::sink::SinkExt;
    use futures::stream::{StreamExt, TryStreamExt};
    use io::Result;
    use std::io::Read;
    use tempfile::tempdir;
    use tokio_util::codec::Decoder;

    #[tokio::test]
    async fn test_tar_from_iter() -> Result<()> {
        let temp_dir = tempdir().unwrap();
        let tar_file_name = temp_dir.path().join("test2.tar");
        let tar_file_name2 = tar_file_name.clone();
        let files = &["README.md", "Cargo.toml"];
        let sizes = files.iter().map(|f| Path::new(f).metadata().unwrap().len());
        let expected_archive_len = calc_size(sizes);
        let tar_stream = TarStream::tar_iter_rel(files.iter(), std::env::current_dir().unwrap());
        let tar_file = tokio_fs::File::create(tar_file_name).await?;
        let codec = tokio_util::codec::BytesCodec::new();
        let mut file_sink = codec.framed(tar_file);
        file_sink
            .send_all(&mut tar_stream.map(|v| v.map(Bytes::from)))
            .await?;

        let archive_len = tar_file_name2.metadata().unwrap().len();
        assert_eq!(
            archive_len, expected_archive_len,
            "archive size is as expected"
        );
        check_archive(tar_file_name2, 2);
        temp_dir.close().unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn test_create_tar() -> Result<()> {
        let temp_dir = tempdir().unwrap();
        let tar_file_name = temp_dir.path().join("test.tar");
        //let tar_file_name = Path::new("/tmp/test.tar");
        let tar_file_name2 = tar_file_name.clone();

        let tar = TarStream::tar_dir(".").await?;
        let tar_file = tokio_fs::File::create(tar_file_name).await?;
        let codec = tokio_util::codec::BytesCodec::new();
        let mut file_sink = codec.framed(tar_file);
        file_sink.send_all(&mut tar.map_ok(Bytes::from)).await?;

        check_archive(tar_file_name2, 2);
        temp_dir.close().unwrap();
        Ok(())
    }

    fn check_archive(p: PathBuf, num_files: usize) {
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

            count += 1;
        }

        assert_eq!(num_files, count, "There are {} files in archive", num_files);
    }

    #[test]
    fn test_cut_path() {
        let a = "abcdef";
        let x = cut_path(a, 10);
        assert_eq!(a, x.to_str().unwrap(), "under limit");

        let a = "0123456789abcd";
        let x = cut_path(a, 10);
        assert_eq!("0123456789", x.to_str().unwrap(), "over limit, no ext");

        let a = "0123456789abcd.mp3";
        let x = cut_path(a, 10);
        assert_eq!("012345.mp3", x.to_str().unwrap(), "over limit, no ext");
    }
}
