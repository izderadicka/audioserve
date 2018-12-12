use super::AudioCodec;

// Opus codec parameters

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
pub struct Opus {
    bitrate: u16,
    compression_level: u8,
    cutoff: Bandwidth,
}

impl Opus {
    pub fn new(bitrate: u16, compression_level: u8, cutoff: Bandwidth) -> Self {
        Opus {
            bitrate,
            compression_level,
            cutoff,
        }
    }
}

impl AudioCodec for Opus {
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

    fn codec_args(&self) -> &'static[&'static str]  {
        &["-acodec",
        "libopus",
        "-vbr",
        "on",]
    }   

    fn bitrate(&self) -> u32 {
        self.bitrate as u32
    }

    
}


