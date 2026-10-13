//! A sound held whole in memory, and reading one from a file.

use std::sync::Arc;

use anyhow::{bail, Context};

/// A sound: frames of `channels` samples each, one after another, at `rate` frames a second.
/// Cheap to clone, since the samples are shared.
#[derive(Clone, Debug)]
pub struct Sound {
    pub rate: u32,
    pub channels: u16,
    pub samples: Arc<[f32]>,
}

impl Sound {
    pub fn new(rate: u32, channels: u16, samples: Vec<f32>) -> Self {
        Self { rate: rate.max(1), channels: channels.max(1), samples: samples.into() }
    }

    /// How long it lasts, in seconds.
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / self.channels as f32 / self.rate as f32
    }

    /// A plain tone that swells and dies away, for a game that has no sounds of its own yet
    /// and for trying things out.
    pub fn tone(hertz: f32, seconds: f32) -> Self {
        let rate = 44_100;
        let frames = (seconds.max(0.0) * rate as f32) as usize;
        let samples = (0..frames)
            .map(|frame| {
                let t = frame as f32 / rate as f32;
                // Eased in and out over a hundredth of a second, so that it does not click.
                let edge = (t / 0.01).min((seconds - t) / 0.01).clamp(0.0, 1.0);
                (t * hertz * std::f32::consts::TAU).sin() * 0.5 * edge
            })
            .collect();
        Self::new(rate, 1, samples)
    }

    /// Reads a sound from a file: a WAV file, or Ogg Vorbis (`.ogg`).
    pub fn load(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let ogg = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("ogg"));
        if ogg { Self::from_ogg(&bytes) } else { Self::from_wav(&bytes) }.with_context(|| format!("in {}", path.display()))
    }

    /// Reads a WAV file's bytes: whole numbers of 8, 16, 24 or 32 bits, or 32-bit floats.
    pub fn from_wav(bytes: &[u8]) -> anyhow::Result<Self> {
        if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            bail!("not a WAV file");
        }
        let number = |at: usize, size: usize| -> anyhow::Result<u32> {
            let part = bytes.get(at..at + size).context("the WAV file ends early")?;
            Ok(part.iter().rev().fold(0, |sum, byte| sum << 8 | *byte as u32))
        };
        // The file is a run of parts, each a four-letter name, a length, and that much.
        let (mut at, mut format, mut data) = (12, None, None);
        while at + 8 <= bytes.len() {
            let (name, length) = (&bytes[at..at + 4], number(at + 4, 4)? as usize);
            let body = at + 8;
            match name {
                b"fmt " => format = Some((number(body, 2)?, number(body + 2, 2)?, number(body + 4, 4)?, number(body + 14, 2)?)),
                b"data" => data = Some(bytes.get(body..(body + length).min(bytes.len())).context("the WAV file ends early")?),
                _ => {}
            }
            // Parts begin on even bytes.
            at = body + length + (length & 1);
        }
        let (Some((kind, channels, rate, bits)), Some(data)) = (format, data) else { bail!("the WAV file has no format or no sound") };
        if channels == 0 || rate == 0 {
            bail!("the WAV file says it has no channels or no rate");
        }
        // 1 is whole numbers and 3 floats; 0xFFFE says the kind is told further on, and is
        // nearly always whole numbers.
        let samples: Vec<f32> = match (kind, bits) {
            (1 | 0xFFFE, 8) => data.iter().map(|byte| (*byte as f32 - 128.0) / 128.0).collect(),
            (1 | 0xFFFE, 16) => data.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32_768.0).collect(),
            (1 | 0xFFFE, 24) => data.as_chunks::<3>().0.iter().map(|b| (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0).collect(),
            (1 | 0xFFFE, 32) => data.as_chunks::<4>().0.iter().map(|b| i32::from_le_bytes(*b) as f32 / 2_147_483_648.0).collect(),
            (3, 32) => data.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect(),
            _ => bail!("a WAV file of kind {kind} with {bits} bits a sample is not one this reads"),
        };
        Ok(Self::new(rate, channels as u16, samples))
    }

    /// Reads the whole of an Ogg Vorbis file's bytes. For music, which is long, see
    /// [`Audio::play_music`](super::Audio::play_music), which reads it as it plays.
    pub fn from_ogg(bytes: &[u8]) -> anyhow::Result<Self> {
        let mut reader = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(bytes)).context("not an Ogg Vorbis file")?;
        let (rate, channels) = (reader.ident_hdr.audio_sample_rate, reader.ident_hdr.audio_channels as u16);
        let mut samples = Vec::new();
        while let Some(packet) = reader.read_dec_packet_itl().context("reading the Ogg Vorbis sound")? {
            samples.extend(packet.into_iter().map(|sample| sample as f32 / 32_768.0));
        }
        Ok(Self::new(rate, channels, samples))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV file of whole numbers, as a recorder would write it.
    fn wav(channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut file = Vec::new();
        file.extend(b"RIFF");
        file.extend((36 + data.len() as u32).to_le_bytes());
        file.extend(b"WAVEfmt ");
        file.extend(16u32.to_le_bytes());
        file.extend(1u16.to_le_bytes());
        file.extend(channels.to_le_bytes());
        file.extend(rate.to_le_bytes());
        file.extend((rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        file.extend((channels * bits / 8).to_le_bytes());
        file.extend(bits.to_le_bytes());
        // A part this does not know, of an odd length, which is to be stepped over.
        file.extend(b"LIST");
        file.extend(3u32.to_le_bytes());
        file.extend([1, 2, 3, 0]);
        file.extend(b"data");
        file.extend((data.len() as u32).to_le_bytes());
        file.extend(data);
        file
    }

    #[test]
    fn a_wav_file_is_read_whatever_its_samples_are_kept_as() {
        let sixteen: Vec<u8> = [0i16, 16_384, -32_768, 32_767].iter().flat_map(|sample| sample.to_le_bytes()).collect();
        let sound = Sound::from_wav(&wav(2, 22_050, 16, &sixteen)).unwrap();
        assert_eq!((sound.rate, sound.channels), (22_050, 2));
        assert_eq!(&sound.samples[..3], &[0.0, 0.5, -1.0]);
        assert!((sound.samples[3] - 1.0).abs() < 1e-4);
        assert!((sound.seconds() - 2.0 / 22_050.0).abs() < 1e-9);

        let eight = Sound::from_wav(&wav(1, 8_000, 8, &[128, 255, 0])).unwrap();
        assert_eq!(&eight.samples[..], &[0.0, 127.0 / 128.0, -1.0]);
        let twenty_four = Sound::from_wav(&wav(1, 48_000, 24, &[0, 0, 0x40, 0, 0, 0xC0])).unwrap();
        assert_eq!(&twenty_four.samples[..], &[0.5, -0.5]);
    }

    #[test]
    fn what_is_not_a_sound_is_refused_and_says_why() {
        assert!(Sound::from_wav(b"not a sound at all").is_err());
        assert!(Sound::from_wav(&wav(1, 8_000, 12, &[0, 0])).unwrap_err().to_string().contains("12 bits"));
        let mut cut = wav(1, 8_000, 16, &[0, 0, 0, 0]);
        cut.truncate(30);
        assert!(Sound::from_wav(&cut).is_err());
        assert!(Sound::from_ogg(b"RIFF....WAVE").is_err());
        assert!(Sound::load("no/such/file.wav").unwrap_err().to_string().contains("no/such/file.wav"));
    }

    #[test]
    fn a_tone_is_as_long_as_asked_and_begins_and_ends_quietly() {
        let tone = Sound::tone(440.0, 0.5);
        assert!((tone.seconds() - 0.5).abs() < 1e-3);
        assert!(tone.samples[0].abs() < 1e-3 && tone.samples[tone.samples.len() - 1].abs() < 1e-2);
        assert!(tone.samples.iter().any(|sample| *sample > 0.45));
    }
}