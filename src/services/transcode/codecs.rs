use super::AudioCodec;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

// Opus codec parameters

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Bandwidth {
    NarrowBand,
    MediumBand,
    WideBand,
    SuperWideBand,
    FullBand,
    Unlimited,
}

impl Bandwidth {
    fn to_hz(&self) -> u16 {
        match *self {
            Bandwidth::NarrowBand => 4000,
            Bandwidth::MediumBand => 6000,
            Bandwidth::WideBand => 8000,
            Bandwidth::SuperWideBand => 12000,
            Bandwidth::FullBand => 20000,
            Bandwidth::Unlimited => 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Opus {
    bitrate: u16,
    compression_level: u8,
    cutoff: Bandwidth,
    #[serde(default)]
    mono: bool,
}

impl Opus {
    pub fn new(bitrate: u16, compression_level: u8, cutoff: Bandwidth, mono: bool) -> Self {
        Opus {
            bitrate,
            compression_level,
            cutoff,
            mono,
        }
    }
}

impl AudioCodec for Opus {
    fn quality_args(&self) -> Vec<Cow<'static, str>> {
        let mut v = vec![];
        if self.mono {
            v.push("-ac".into());
            v.push("1".into());
        }
        v.push("-b:a".into());
        v.push(format!("{}k", self.bitrate).into());
        v.push("-compression_level".into());
        v.push(format!("{}", self.compression_level).into());
        v.push("-cutoff".into());
        v.push(format!("{}", self.cutoff.to_hz()).into());
        v
    }

    fn codec_args(&self) -> &'static [&'static str] {
        &["-acodec", "libopus", "-vbr", "on"]
    }

    fn bitrate(&self) -> u32 {
        u32::from(self.bitrate)
    }
}

// MP3 codec
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Mp3 {
    bitrate: u32,
    /// Quality 0 best 9 worst - opposite of opus!
    compression_level: u8,
    /// ABR = average bit rate - something like variable bit rate
    #[serde(default)]
    abr: bool,
    #[serde(default)]
    mono: bool,
}

impl AudioCodec for Mp3 {
    fn quality_args(&self) -> Vec<Cow<'static, str>> {
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
        v.push(format!("{}k", self.bitrate).into());
        v.push("-compression_level".into());
        v.push(format!("{}", self.compression_level).into());

        v
    }

    fn codec_args(&self) -> &'static [&'static str] {
        &["-acodec", "libmp3lame"]
    }

    fn bitrate(&self) -> u32 {
        self.bitrate
    }
}

// AAC codec
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
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
    #[default]
    Unlimited,
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
            SampleRate::Unlimited => 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Aac {
    bitrate: u32,
    #[serde(default)]
    sr: SampleRate,
    #[serde(default)]
    ltp: bool,
    #[serde(default)]
    mono: bool,
}

impl AudioCodec for Aac {
    fn quality_args(&self) -> Vec<Cow<'static, str>> {
        let mut v = vec![];
        if self.mono {
            v.push("-ac".into());
            v.push("1".into());
        }
        if self.sr != SampleRate::Unlimited {
            v.push("-ar".into());
            v.push(self.sr.to_sr().to_string().into())
        }
        v.push("-b:a".into());
        v.push(format!("{}k", self.bitrate).into());
        v.push("-aac_coder".into());
        v.push("twoloop".into());
        if self.ltp {
            v.push("-aac_ltp".into());
            v.push("1".into());
        }
        v
    }

    fn codec_args(&self) -> &'static [&'static str] {
        &["-strict", "-2", "-acodec", "aac"]
    }

    fn bitrate(&self) -> u32 {
        self.bitrate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bandwidth_to_hz() {
        assert_eq!(Bandwidth::NarrowBand.to_hz(), 4000);
        assert_eq!(Bandwidth::MediumBand.to_hz(), 6000);
        assert_eq!(Bandwidth::WideBand.to_hz(), 8000);
        assert_eq!(Bandwidth::SuperWideBand.to_hz(), 12000);
        assert_eq!(Bandwidth::FullBand.to_hz(), 20000);
        assert_eq!(Bandwidth::Unlimited.to_hz(), 0);
    }

    #[test]
    fn test_sample_rate_to_sr() {
        assert_eq!(SampleRate::_8kHz.to_sr(), 8_000);
        assert_eq!(SampleRate::_12kHz.to_sr(), 12_000);
        assert_eq!(SampleRate::_16kHz.to_sr(), 16_000);
        assert_eq!(SampleRate::_24kHz.to_sr(), 24_000);
        assert_eq!(SampleRate::_32kHz.to_sr(), 32_000);
        assert_eq!(SampleRate::_48kHz.to_sr(), 48_000);
        assert_eq!(SampleRate::Unlimited.to_sr(), 0);
    }

    #[test]
    fn test_opus_codec_args() {
        let opus = Opus::new(48, 8, Bandwidth::FullBand, false);
        assert!(opus.codec_args().contains(&"libopus"));
        let quality = opus.quality_args();
        assert!(!quality.is_empty());
        let joined: Vec<&str> = quality.iter().map(|s| s.as_ref()).collect();
        assert!(joined.contains(&"-b:a"));
        assert!(joined.contains(&"48k"));
        assert!(joined.contains(&"-compression_level"));
        assert!(joined.contains(&"-cutoff"));
    }

    #[test]
    fn test_mp3_codec_args() {
        let mp3 = Mp3 {
            bitrate: 128,
            compression_level: 5,
            abr: false,
            mono: false,
        };
        assert!(mp3.codec_args().contains(&"libmp3lame"));
        let quality = mp3.quality_args();
        assert!(!quality.is_empty());
        let joined: Vec<&str> = quality.iter().map(|s| s.as_ref()).collect();
        assert!(joined.contains(&"-b:a"));
        assert!(joined.contains(&"128k"));
    }
}
