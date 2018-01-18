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
    /// app. 1.5 * transcoding bitrate (bandwidth saving should outweight transoding costs)
    fn transcode_from(&self) -> u32 {
        match self {
            &Quality::Low => 48,
            &Quality::Medium => 64,
            &Quality::High => 96,
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

    fn build_command<S: AsRef<OsStr>>(&self, file: S, seek:Option<f32>) -> Command {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(&["-nostdin", "-v", "error"]);
        if let Some(s) = seek {
            cmd.args(&["-accurate_seek", "-ss"]);
            let time_spec = format!("{:2}",s);
            cmd.arg(time_spec);
        }
        cmd
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

    pub fn should_transcode(&self, bitrate: u32, mime: &Mime) -> bool {
        if super::types::must_transcode(mime) {
            return true;
        }
        trace!(
            "Should transcode {} > {}",
            bitrate,
            self.quality.transcode_from()
        );
        bitrate > self.quality.transcode_from()
    }

    pub fn transcoded_mime(&self) -> Mime {
        "audio/ogg".parse().unwrap()
    }

    pub fn transcode<S: AsRef<OsStr>>(
        &self,
        file: S,
        seek: Option<f32>,
        mut body_tx: futures::sync::mpsc::Sender<Result<hyper::Chunk, hyper::Error>>,
    ) {
        let mut cmd = self.build_command(&file, seek);
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
                            debug!("finished sending transcoded data");
                            break;
                        } else {
                            let slice = buf[..n].to_vec();
                            let c: Chunk = slice.into();
                            trace!("Sending {} bytes", n);
                            match body_tx.send(Ok(c)).wait() {
                                Ok(t) => body_tx = t,
                                Err(_) => { 
                                    warn!("Cannot send data to response stream");
                                    break
                                    },
                            };

                        },
                        Err(e) => {
                            error!("Stdout read error {:?}", e);
                            break
                            },
                    };
                }
                // if preliminary_end {
                    
                //     debug!("Ending preliminary, need to kill transcoding process");
                //     child.kill().unwrap_or_else(|e| error!{"Cannot kill process: {}", e});
                // }

                //must drop out to close subprocess stdout
                drop(out);
                debug!("waiting for transcode process to end");
                match child.wait() {
                    Ok(status) => if !status.success() {
                        warn!("Transconding of file {:?} failed with code {:?}", file.as_ref(), status.code())
                    } else {
                        debug!("Finished transcoding process normally")
                    },
                    Err(e) => error!("Cannot get process status: {}", e)
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
    //use pretty_env_logger;
    use std::path::Path;

    fn dummy_transcode<P: AsRef<Path>>(output_file: P, seek:Option<f32>) {
        //pretty_env_logger::init().unwrap();
        let t = Transcoder::new(Quality::Low);
        let out_file = temp_dir().join(output_file);
        let mut cmd = t.build_command("./test_data/01-file.mp3", seek);
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
        assert!(out_file.exists());
        //TODO: for some reasons sometimes cannot get meta - but file is OK
        if let Some(meta) = get_audio_properties(&out_file) {
            let dur = 2 - seek.map(|s| s.round() as u32).unwrap_or(0);
        assert_eq!(meta.duration, dur);
        }
        remove_file(&out_file).expect("error deleting tmp file");

    }

    #[test]
    fn test_transcode() {
        dummy_transcode("audioserve_transcoded.opus",None)
    }

    #[test]
    fn test_transcode_seek() {
        dummy_transcode("audioserve_transcoded2.opus",Some(0.8))
    }

    
}
