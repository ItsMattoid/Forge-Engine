//! Decoding sound files (WAV, MP3, FLAC, OGG Vorbis, M4A / AAC) into clips the mixer can play.
//!
//! A clip is the whole sound as 16-bit stereo in memory: sounds are short (effects, voices) or, for music, a few tens of
//! megabytes - and keeping them decoded makes seeking, looping and playing the same sound many times at once free.
//! Mono files are played on both sides; files with more than two channels keep their first two.

use std::io::Cursor;

use rodio::Source;

/// A decoded sound.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// Samples per second of the file.
    pub rate: u32,
    /// Interleaved left / right samples.
    pub samples: Vec<i16>,
}

/// A file longer than this is refused (a corrupt header must not make the engine decode for minutes or fill the memory).
pub const MAX_SECONDS: f32 = 3.0 * 3600.0;

impl Clip {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.rate.max(1) as f32
    }

    /// Both channels at a frame as numbers in -1 ..= 1.
    pub fn frame(&self, i: usize) -> (f32, f32) {
        let k = i * 2;
        match (self.samples.get(k), self.samples.get(k + 1)) {
            (Some(l), Some(r)) => (f32::from(*l) / 32768.0, f32::from(*r) / 32768.0),
            _ => (0.0, 0.0),
        }
    }

    /// The memory the clip takes.
    pub fn bytes(&self) -> usize {
        self.samples.len() * 2
    }

    /// A clip made of numbers (the tests, and sounds a game makes up).
    pub fn from_mono(rate: u32, samples: &[f32]) -> Clip {
        let mut out = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            out.extend([v, v]);
        }
        Clip { rate, samples: out }
    }
}

/// Reads a sound file's bytes.
pub fn decode(bytes: Vec<u8>) -> Result<Clip, String> {
    if bytes.is_empty() {
        return Err("the file is empty".into());
    }
    let decoder = rodio::Decoder::new(Cursor::new(bytes)).map_err(|e| format!("the sound cannot be read: {e}"))?;
    let channels = usize::from(decoder.channels().get());
    let rate = decoder.sample_rate().get();
    if channels == 0 || rate == 0 {
        return Err("the sound has no channels".into());
    }
    let limit = (MAX_SECONDS * rate as f32) as usize * channels;
    let mut samples: Vec<i16> = Vec::new();
    let mut frame = [0f32; 2];
    let mut k = 0usize;
    for (n, s) in decoder.enumerate() {
        if n > limit {
            return Err(format!("the sound is longer than {} hours", MAX_SECONDS / 3600.0));
        }
        let slot = k % channels;
        if slot < 2 {
            frame[slot] = s;
        }
        k += 1;
        if slot == channels - 1 {
            if channels == 1 {
                frame[1] = frame[0];
            }
            for v in frame {
                samples.push((v.clamp(-1.0, 1.0) * 32767.0).round() as i16);
            }
        }
    }
    if samples.is_empty() {
        return Err("the sound has no samples".into());
    }
    Ok(Clip { rate, samples })
}

/// Whether bytes look like a sound the decoder will open (its header is read; the samples are not decoded).
pub fn probe(bytes: Vec<u8>) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("the file is empty".into());
    }
    rodio::Decoder::new(Cursor::new(bytes)).map(|_| ()).map_err(|e| format!("the sound cannot be read: {e}"))
}

/// The file extensions [`decode`] understands.
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "ogg", "flac", "m4a", "aac"];

pub fn is_audio(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    AUDIO_EXTENSIONS.iter().any(|e| lower.ends_with(&format!(".{e}")))
}

/// A WAV file of 16-bit mono samples (for tests and tools).
pub fn wav_bytes(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mono_wav_is_played_on_both_sides_and_its_length_is_known() {
        let samples: Vec<i16> = (0..4410).map(|i| ((i as f32 * 0.05).sin() * 8000.0) as i16).collect();
        let clip = decode(wav_bytes(44100, 1, &samples)).unwrap();
        assert_eq!((clip.rate, clip.frames()), (44100, 4410));
        assert!((clip.seconds() - 0.1).abs() < 1e-4);
        let (l, r) = clip.frame(100);
        assert_eq!(l, r);
        assert!((l - f32::from(samples[100]) / 32768.0).abs() < 2e-4, "{l}");
        assert_eq!(clip.frame(99_999), (0.0, 0.0), "past the end is silence");
    }

    #[test]
    fn a_stereo_wav_keeps_its_sides() {
        let mut interleaved = Vec::new();
        for i in 0..200 {
            interleaved.extend([1000i16 + i, -1000 - i]);
        }
        let clip = decode(wav_bytes(22050, 2, &interleaved)).unwrap();
        assert_eq!(clip.frames(), 200);
        let (l, r) = clip.frame(10);
        assert!(l > 0.0 && r < 0.0, "{l} {r}");
    }

    #[test]
    fn files_that_are_not_sounds_are_errors_not_panics() {
        assert!(decode(Vec::new()).is_err());
        assert!(decode(b"this is not audio at all".to_vec()).is_err());
        let mut cut = wav_bytes(44100, 1, &vec![100i16; 1000]);
        cut.truncate(30);
        assert!(decode(cut).is_err());
        assert!(is_audio("A.MP3") && is_audio("x.ogg") && !is_audio("x.png"));
    }

    #[test]
    fn a_clip_made_from_numbers_is_clamped() {
        let c = Clip::from_mono(8000, &[0.0, 1.0, -1.0, 5.0]);
        assert_eq!(c.frame(1), (32767.0 / 32768.0, 32767.0 / 32768.0));
        assert_eq!(c.frame(3).0, c.frame(1).0);
        assert_eq!(c.bytes(), 16);
    }
}
