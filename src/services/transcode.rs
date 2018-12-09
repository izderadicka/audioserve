use config::get_config;
use error::Error;
use futures::future::{Either};
use futures::Future;
use mime::Mime;
use services::subs::ChunkStream;
use std::ffi::OsStr;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use std::fmt::Debug;
use tokio::timer::Delay;
use tokio_process::{ChildStdout, CommandExt};


#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Bandwidth {
    NarrowBand,
    MediumBand,
    WideBand,
    SuperWideBand,
    FullBand,
}

impl Bandwidth {
    fn to_hz(&self) -> u16 {
        match *self {
            Bandwidth::NarrowBand => 4000,
            Bandwidth::MediumBand => 6000,
            Bandwidth::WideBand => 8000,
            Bandwidth::SuperWideBand => 12000,
            Bandwidth::FullBand => 20000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Quality {
    bitrate: u16,
    compression_level: u8,
    cutoff: Bandwidth,
}

#[derive(Clone, Debug, PartialEq, Copy)]
pub enum QualityLevel {
    Low,
    Medium,
    High,
}

impl QualityLevel {
    pub fn from_letter<T: AsRef<str>>(l: &T) -> Option<Self> {
        use self::QualityLevel::*;
        let s: &str = l.as_ref();
        match s {
            "l" => Some(Low),
            "m" => Some(Medium),
            "h" => Some(High),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn to_letter(&self) -> &'static str {
        use self::QualityLevel::*;
        match *self {
            Low => "l",
            Medium => "m",
            High => "h"
        }
    }
}

impl Quality {
    fn new(bitrate: u16, compression_level: u8, cutoff: Bandwidth) -> Self {
        Quality {
            bitrate,
            compression_level,
            cutoff,
        }
    }

    pub fn default_level(l: QualityLevel) -> Self {
        match l {
            QualityLevel::Low => Quality::new(32, 5, Bandwidth::SuperWideBand),
            QualityLevel::Medium => Quality::new(48, 8, Bandwidth::SuperWideBand),
            QualityLevel::High => Quality::new(64, 10, Bandwidth::FullBand),
        }
    }

    fn quality_args(&self) -> Vec<String> {
        let mut v = vec![];
        v.push("-b:a".into());
        v.push(format!("{}k", self.bitrate));
        v.push("-compression_level".into());
        v.push(format!("{}", self.compression_level));
        v.push("-cutoff".into());
        v.push(format!("{}", self.cutoff.to_hz()));
        v
    }

    /// Bitrate from which it make sense to transcode - kbps

    fn transcode_from(&self) -> u32 {
        (f32::from(self.bitrate) * 1.2) as u32
    }
}

#[derive(Clone,Debug)]
pub enum AudioFilePath<S> {
    Original(S),
    #[allow(dead_code)]
    Transcoded(S)
}

impl <S> std::convert::AsRef<S> for AudioFilePath<S> {
    fn as_ref(&self) -> &S {
        use self::AudioFilePath::*;
        match self {
            Original(ref f) => f,
            Transcoded(ref f) => f
        }
    }
}

#[derive(Clone, Debug)]
pub struct Transcoder {
    quality: Quality,
}

#[cfg(feature = "transcoding-cache")]
type TranscodedStream = Box<dyn futures::Stream<Item=Vec<u8>, Error=std::io::Error>+Send+'static>;
#[cfg(feature = "transcoding-cache")]
type TranscodedFuture = Box<dyn Future<Item=TranscodedStream, Error=Error>+Send>;



impl Transcoder {
    pub fn new(quality: Quality) -> Self {
        Transcoder { quality }
    }

    // ffmpeg -nostdin -v error -i 01-file.mp3 -y -map_metadata 0 -map a -acodec libopus \
    // -b:a 48k -vbr on -compression_level 10 -application audio -cutoff 12000 -f opus pipe:1
    fn build_command<S: AsRef<OsStr>>(&self, file: S, seek: Option<f32>) -> Command {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(&["-nostdin", "-v", "error"]);
        if let Some(s) = seek {
            cmd.args(&["-accurate_seek", "-ss"]);
            let time_spec = format!("{:2}", s);
            cmd.arg(time_spec);
        }
        cmd.arg("-i")
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

    // should not transcode, just copy audio stream
    #[allow(dead_code)]
    fn build_copy_command<S: AsRef<OsStr>>(&self, file: S, seek: Option<f32>) -> Command {
        let mut cmd = Command::new("ffmpeg");
        cmd.args(&["-nostdin", "-v", "error"]);
        if let Some(s) = seek {
            cmd.args(&["-accurate_seek", "-ss"]);
            let time_spec = format!("{:2}", s);
            cmd.arg(time_spec);
        }
        cmd.arg("-i")
            .arg(file)
            .args(&[
                "-y",
                "-map_metadata",
                "0",
                "-map",
                "a",
                "-acodec",
                "copy",
                
            ])
            .args(&["-f", "opus", "pipe:1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        cmd
    }

    pub fn transcoding_params(&self) -> String {
        format!(
            "codec=opus; bitrate={}; compression_level={}; cutoff={}",
            self.quality.bitrate,
            self.quality.compression_level,
            self.quality.cutoff.to_hz()
        )
    }

    //TODO - keeping it for a while if we need to check clients
    #[allow(dead_code)]
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

    pub fn transcoded_mime() -> Mime {
        "audio/ogg".parse().unwrap()
    }

    #[cfg(not(feature = "transcoding-cache"))]
    pub fn transcode<S: AsRef<OsStr> + Send + Debug+ 'static>(
        self,
        file: AudioFilePath<S>,
        seek: Option<f32>,
        counter: super::Counter,
        _quality: QualityLevel
    ) -> impl Future<Item=ChunkStream<ChildStdout>, Error=Error> {
        futures::future::result(self.transcode_inner(file, seek, counter)
        .map(|(stream, f)| {
            tokio::spawn(f);
            stream
        })
        )
    }

    #[cfg(feature = "transcoding-cache")]
    pub fn transcode<S: AsRef<OsStr> + Debug+ Send + 'static>(
        self,
        file: AudioFilePath<S>,
        seek: Option<f32>,
        counter: super::Counter,
        quality: QualityLevel
    ) -> TranscodedFuture {

        use crate::cache::{cache_key, get_cache};
        use futures::future;
        use futures::sync::mpsc;
        use futures::{Stream,Sink};
        use std::io;


        if seek.is_some() || get_config().transcoding_cache.disabled {
            debug!("Shoud not add to cache as seeking or cache is disabled");
            return Box::new(future::result(
            self.transcode_inner(file, seek, counter)
            .map(|(stream, f)| {
                    tokio::spawn(f);
                    Box::new(stream) as TranscodedStream
            })))
        }
        

        let cache = get_cache();
        //TODO: this is ugly -  unify either we will use Path or OsStr!
        let key = cache_key(file.as_ref().as_ref(), &quality);
        let fut = cache.add_async(key).then( move |res| {
            match res {
                Err(e) => {
                    warn!("Cannot create cache entry: {}", e);
                    self.transcode_inner(file, seek, counter)
                    .map(|(stream, f)| {
                        tokio::spawn(f);
            
                    Box::new(stream) as TranscodedStream
                    })
                },
                Ok((cache_file, cache_finish)) => {
                    self.transcode_inner(file, seek, counter)
                    .map(|(stream, f)| {
                        tokio::spawn(f.then(|res| {

                            fn box_me<I,E,F:Future<Item=I, Error=E>+'static+Send>(f: F ) -> 
                            Box<Future<Item=I, Error=E>+'static+Send> {
                                Box::new(f)
                            };

                            match res {
                                Ok(()) => box_me(cache_finish.commit()
                                    .map_err(|e| error!("Error in cache: {}", e))
                                    .and_then(|_| {
                                        debug!("Added to cache"); 
                                        get_cache().save_index_async()
                                        .map_err(|e| error!("Error when saving cache index: {}", e))
                                        })),
                                Err(()) => box_me(cache_finish.roll_back()
                                    .map_err(|e| error!("Error in cache: {}", e))),
                            }
                            

                            
                        }
                        ));
                    let cache_sink = tokio::codec::FramedWrite::new(cache_file, self::vec_codec::VecEncoder);
                    let (tx,rx) = mpsc::channel(64);
                    let tx = cache_sink.fanout(tx.sink_map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
                    tokio::spawn(tx.send_all(stream)
                    .then(|res| {
                        if let Err(e) = res {
                            warn!("Error in channel: {}",e)
                        }
                        Ok(())
                    }));
                    Box::new(rx.map_err(|_| {
                        error!("Error in chanel");
                        io::Error::new(io::ErrorKind::Other, "Error in channel")
                    })) as TranscodedStream
                    })
                }
            }
        });
        Box::new(fut)
    }

    fn transcode_inner<S: AsRef<OsStr> + Debug+ Send + 'static>(
        &self,
        file: AudioFilePath<S>,
        seek: Option<f32>,
        counter: super::Counter,
    ) -> Result<(ChunkStream<ChildStdout>, impl Future<Item=(), Error=()>), Error> {
        let mut cmd = match file {
            AudioFilePath::Original(ref file) => self.build_command(file, seek),
            AudioFilePath::Transcoded(ref file) => self.build_command(file, seek)
        };
        let counter2 = counter.clone();
        match cmd.spawn_async() {
            Ok(mut child) => {
                if child.stdout().is_some() {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let start = Instant::now();
                    let mut out = child.stdout().take().unwrap();
                    let stream = ChunkStream::new(out);
                    let pid = child.id();
                    debug!("waiting for transcode process to end");
                    let fut = 
                    
                    child
                        .select2(Delay::new(
                            Instant::now()
                                + Duration::from_secs(
                                    u64::from(get_config().transcoding_deadline * 3600),
                                ),
                        ))
                        .then(move |res| {
                            counter2.fetch_sub(1, Ordering::SeqCst);
                            match res {
                                Ok(Either::A((res, _d))) => {
                                    if res.success() {
                                        debug!("Finished transcoding process of {:?} normally after {:?}",
                                    file.as_ref(),
                                    Instant::now() - start);
                                    Ok(())
                                    } else {
                                        warn!(
                                            "Transconding of file {:?} failed with code {:?}",
                                            file.as_ref(),
                                            res.code()
                                        );
                                        Err(())
                                    }
                                   
                                }
                                Ok(Either::B((_d, mut child))) => {
                                    eprintln!(
                                        "Transcoding of file {:?} took longer then deadline",
                                        file.as_ref()
                                    );
                                    child.kill().unwrap_or_else(|e| {
                                        eprintln!("Failed to kill process pid {} error {}", pid, e)
                                    });
                                    Err(())
                                }
                                Err(Either::A((e, _))) => {
                                    eprintln!(
                                        "Error running transcoding process for file {:?} error {}",
                                        file.as_ref(),
                                        e
                                    );
                                    Err(())
                                }
                                Err(Either::B((e, _))) => {
                                    eprintln!("Timer error on process pid {} error {}", pid, e);
                                    Err(())
                                }
                            }
                        });
                    Ok((stream, fut))
                } else {
                    error!("Cannot get stdout");
                    Err(Error::new())
                }
            }
            Err(e) => {
                error!("Cannot spawn child process: {:?}", e);
                Err(Error::new())
            }
        }
    }
}

#[cfg(feature="transcoding-cache")]
mod vec_codec {
    use tokio::codec::Encoder;
    use std::io;
    use bytes::{BufMut};

    pub struct VecEncoder;

    impl Encoder for VecEncoder {
        type Item = Vec<u8>;
        type Error = io::Error;

    fn encode(&mut self, data: Self::Item, buf: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        buf.reserve(data.len());
        buf.put(data);
        Ok(())
    }


    }
}

#[cfg(test)]
mod tests {
    use super::super::subs::get_audio_properties;
    use super::*;
    use std::env::temp_dir;
    use std::fs::{remove_file, File};
    use std::io::{Read, Write};
    use std::path::Path;

    fn dummy_transcode<P: AsRef<Path>, R: AsRef<Path>>(output_file: P, seek: Option<f32>, 
        copy_file: Option<R>, remove: bool) {
        pretty_env_logger::try_init().ok();
        let t = Transcoder::new(Quality::default_level(QualityLevel::Low));
        let out_file = temp_dir().join(output_file);
        let mut cmd = match copy_file {
            None => t.build_command("./test_data/01-file.mp3", seek),
            Some(ref p) => t.build_copy_command(p.as_ref(), seek)
        };
        let mut child = cmd.spawn().expect("Cannot spawn subprocess");

        if child.stdout.is_some() {
            let mut file = File::create(&out_file).expect("Cannot create output file");
            let mut buf = [0u8; 1024];
            let mut out = child.stdout.take().unwrap();
            loop {
                match out.read(&mut buf) {
                    Ok(n) => {
                        if n == 0 {
                            break;
                        } else {
                            file.write_all(&mut buf).expect("Write to file error")
                        }
                    }
                    Err(e) => panic!("stdout read error {:?}", e),
                }
            }
        }
        let status = child.wait().expect("cannot get status");
        assert!(status.success());
        assert!(out_file.exists());
        //TODO: for some reasons sometimes cannot get meta - but file is OK
        if let Some(meta) = get_audio_properties(&out_file) {
            let audio_len = if copy_file.is_some() {1} else {2};
            let dur = audio_len - seek.map(|s| s.round() as u32).unwrap_or(0);
            assert_eq!(meta.duration, dur);
        }
        if remove { 
            remove_file(&out_file).expect("error deleting tmp file");
        }
    }

    #[test]
    fn test_transcode() {
        dummy_transcode("audioserve_transcoded.opus", None, None as Option<&str>, true)
    }

    #[test]
    fn test_transcode_seek() {
        dummy_transcode("audioserve_transcoded2.opus", Some(0.8), None as Option<&str>, false);
        let out_file = temp_dir().join("audioserve_transcoded2.opus");
        dummy_transcode("audioserve_transcoded3.opus", Some(0.8), Some(&out_file), true);
        remove_file(out_file).unwrap();

    }

    

}
