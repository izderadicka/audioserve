use std::process::{Command, Stdio};
use std::ffi::OsStr;
use mime::Mime;
use futures::{self, Sink, Future};
use hyper::{self, Chunk};
use std::io::Read;


#[derive(Clone, Debug)]
pub enum Quality {
    Low,
    Medium,
    High,
}

// ffmpeg -nostdin -v error -i 01-file.mp3 -y -map_metadata 0 -map a -acodec libopus \
// -b:a 48k -vbr on -compression_level 10 -application audio -cutoff 12000 -f opus pipe:1

const LOW_QUALITY_ARGS: &[&str] = &["-b:a", "32k", "-compression_level", "5", "-cutoff", "12000"];
const MEDIUM_QUALITY_ARGS: &[&str] =
    &["-b:a", "48k", "-compression_level", "8", "-cutoff", "12000"];
const HIGH_QUALITY_ARGS: &[&str] = &[
    "-b:a",
    "64k",
    "-compression_level",
    "10",
    "-cutoff",
    "20000",
];

impl Quality {
    fn quality_args(&self) -> &'static [&'static str] {
        match self {
            &Quality::Low => LOW_QUALITY_ARGS,
            &Quality::Medium => MEDIUM_QUALITY_ARGS,
            &Quality::High => HIGH_QUALITY_ARGS,
        }
    }

    /// Bitrate from which it make sense to transcode - kbps
    fn transcode_from(&self) -> u32 {
        match self {
            &Quality::Low => 32,
            &Quality::Medium => 48,
            &Quality::High => 64,
        }
    }
}


#[derive(Clone, Debug)]
pub struct Transcoder {
    quality: Quality,
}


impl Transcoder {
    pub fn new(quality: Quality) -> Self {
        Transcoder { quality }
    }

    fn build_command<S: AsRef<OsStr>>(&self, file: S) -> Command {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(&["-nostdin", "-v", "error"])
            .arg("-i")
            .arg(file)
            .args(&[
                "-y",
                "-map_metadata",
                "0",
                "-map",
                "a",
                "-acodec",
                "libopus",
                "-vbr",
                "on",
            ])
            .args(self.quality.quality_args())
            .args(&["-f", "opus", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        cmd
    }

    pub fn should_transcode(&self, bitrate: u32) -> bool {
        debug!(
            "Should transcode {} >= {}",
            bitrate,
            self.quality.transcode_from()
        );
        bitrate >= self.quality.transcode_from()
    }

    pub fn transcoded_mime(&self) -> Mime {
        "audio/ogg".parse().unwrap()
    }

    pub fn transcode<S: AsRef<OsStr>>(
        &self,
        file: S,
        mut body_tx: futures::sync::mpsc::Sender<Result<hyper::Chunk, hyper::Error>>,
    ) {
        let mut cmd = self.build_command(file);
        match cmd.spawn() {
            Ok(mut child) => if child.stdout.is_some() {
                let mut buf = [0u8; 1024*8];
                let mut out = child.stdout.take().unwrap();
                loop {
                    match out.read(&mut buf) {
                        Ok(n) => if n == 0 {
                            body_tx
                            .close()
                            .map(|_| ())
                            .unwrap_or_else(|e| error!("Cannot close sink {:?}", e));
                            break;
                        } else {
                            let slice = buf[..n].to_vec();
                            let c: Chunk = slice.into();
                            match body_tx.send(Ok(c)).wait() {
                                Ok(t) => body_tx = t,
                                Err(_) => break,
                            };

                        },
                        Err(e) => error!("Stdout read error {:?}", e),
                    }
                }
            } else {
                error!("Cannot get stdout")
            },
            Err(e) => error!("Cannot spawn child process: {:?}", e),
        }

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{remove_file, File};
    use std::io::{Read, Write};
    use super::super::subs::get_audio_properties;
    use std::env::temp_dir;

    #[test]
    fn test_transcode() {
        let t = Transcoder::new(Quality::Low);
        let out_file = temp_dir().join("audioserve_transcoded.opus");
        let mut cmd = t.build_command("./test_data/01-file.mp3");
        let mut child = cmd.spawn().expect("Cannot spawn subprocess");

        if child.stdout.is_some() {
            let mut file = File::create(&out_file).expect("Cannot create output file");
            let mut buf = [0u8; 1024];
            let mut out = child.stdout.take().unwrap();
            loop {
                match out.read(&mut buf) {
                    Ok(n) => if n == 0 {
                        break;
                    } else {
                        file.write_all(&mut buf).expect("Write to file error")
                    },
                    Err(e) => panic!("stdout read error {:?}", e),
                }
            }
        }
        let status = child.wait().expect("cannot get status");
        assert!(status.success());
        let meta = get_audio_properties(&out_file).expect("Cannot get audio metadata");
        assert_eq!(meta.duration, 2);
        remove_file(&out_file).expect("error deleting tmp file");
    }
}
