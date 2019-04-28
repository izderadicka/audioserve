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
    Unlimited
}

impl Bandwidth {
    fn to_hz(&self) -> u16 {
        match *self {
            Bandwidth::NarrowBand => 4000,
            Bandwidth::MediumBand => 6000,
            Bandwidth::WideBand => 8000,
            Bandwidth::SuperWideBand => 12000,
            Bandwidth::FullBand => 20000,
            Bandwidth::Unlimited => 0
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Opus {
    bitrate: u16,
    compression_level: u8,
    cutoff: Bandwidth,
    #[serde(default)]
    mono: bool
}

impl Opus {
    pub fn new(bitrate: u16, compression_level: u8, cutoff: Bandwidth, mono: bool) -> Self {
        Opus {
            bitrate,
            compression_level,
            cutoff,
            mono
        }
    }
}

impl AudioCodec for Opus {
    fn quality_args(&self) -> Vec<String> {
        let mut v = vec![];
        if self.mono {
            v.push("-ac".into());
            v.push("1".into());
        }
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
        u32::from(self.bitrate)
    }

    
}

// MP3 codec
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Mp3 {
    bitrate: u32,
    /// Quality 0 best 9 worst - opposite of opus!
    compression_level: u8,
    /// ABR = average bit rate - something like variable bit rate
    #[serde(default)]
    abr: bool,
    #[serde(default)]
    mono: bool
}

impl AudioCodec for Mp3 {
    fn quality_args(&self) -> Vec<String> {
        let mut v = vec![];
        if self.mono {
            v.push("-ac".into());
            v.push("1".into());
        }
        if self.abr {
            v.push("-abr".into());
            v.push("1".into());
        }
        v.push("-b:a".into());
        v.push(format!("{}k", self.bitrate));
        v.push("-compression_level".into());
        v.push(format!("{}", self.compression_level));
        
        v
    }

    fn codec_args(&self) -> &'static[&'static str]  {
        &["-acodec",
        "libmp3lame"]
    }   

    fn bitrate(&self) -> u32 {
        self.bitrate as u32
    }

    
}

// AAC codec
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum SampleRate {
     #[serde(rename = "8kHz")]
    _8kHz,
     #[serde(rename = "12kHz")]
    _12kHz,
     #[serde(rename = "16kHz")]
    _16kHz,
     #[serde(rename = "24kHz")]
    _24kHz,
     #[serde(rename = "32kHz")]
    _32kHz,
     #[serde(rename = "48kHz")]
    _48kHz,
     #[serde(rename = "unlimited")]
    Unlimited
}

impl SampleRate {
    fn to_sr(&self) -> u32 {
        use self::SampleRate::*;
        match self {
           
            _8kHz => 8_000,
            _12kHz => 12_000,
            _16kHz => 16_000,
            _24kHz => 24_000,
            _32kHz => 32_000,
            _48kHz => 48_000,
            SampleRate::Unlimited => 0
        }
    }
}

impl Default for SampleRate {
    fn default() -> Self {
        SampleRate::Unlimited
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Aac {
    bitrate: u32,
    #[serde(default)]
    sr: SampleRate,
    #[serde(default)]
    ltp: bool,
    #[serde(default)]
    mono: bool
}


impl AudioCodec for Aac {
    fn quality_args(&self) -> Vec<String> {
        let mut v = vec![];
        if self.mono {
            v.push("-ac".into());
            v.push("1".into());
        }
        if self.sr != SampleRate::Unlimited {
            v.push("-ar".into());
            v.push(self.sr.to_sr().to_string())
        }
        v.push("-b:a".into());
        v.push(format!("{}k", self.bitrate));
        v.push("-aac_coder".into());
        v.push("twoloop".into());
        if self.ltp {
            v.push("-aac_ltp".into());
            v.push("1".into());
        }
        v
    }

    fn codec_args(&self) -> &'static[&'static str]  {
        &["-strict", "-2",  "-acodec", "aac"]
    }   

    fn bitrate(&self) -> u32 {
        self.bitrate as u32
    }

}






