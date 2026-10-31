//! The audio engine: the [`Mixer`] and the sound card it plays on.
//!
//! The sound card is opened the first time a sound is played (a game or an editor that never plays one never touches the
//! audio device). If there is none, or it cannot be opened - or `FORGE_AUDIO=off` is set, as the tests and build servers
//! do - the engine still runs: it renders the mix into a scratch buffer as time passes, so sounds still progress, end and
//! report it exactly as they would be heard. The reason is kept in [`AudioEngine::device_error`] for the debug overlay.

use std::{
    num::NonZero,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use rodio::Source;

use super::mixer::Mixer;

/// The rate the mix is made at when there is no sound card.
const OFFLINE_RATE: u32 = 48_000;
/// Frames the audio thread asks the mixer for at a time.
const BLOCK_FRAMES: usize = 512;

pub struct AudioEngine {
    shared: Arc<Mutex<Mixer>>,
    sink: Option<rodio::MixerDeviceSink>,
    /// The sound card was tried (and either works or `device_error` says why not).
    tried: bool,
    pub device_error: Option<String>,
    scratch: Vec<f32>,
    /// Never open the sound card (the tests).
    offline: bool,
}

impl Default for AudioEngine {
    fn default() -> Self {
        AudioEngine::new()
    }
}

impl AudioEngine {
    /// An engine that opens the sound card when a sound first plays.
    pub fn new() -> AudioEngine {
        let off = std::env::var("FORGE_AUDIO").is_ok_and(|v| v.eq_ignore_ascii_case("off") || v == "0");
        let mut e = AudioEngine::offline();
        e.offline = off;
        if off {
            e.tried = true;
            e.device_error = Some("FORGE_AUDIO=off".into());
        }
        e
    }

    /// An engine with no sound card: the mix only advances when [`AudioEngine::update`] is called.
    pub fn offline() -> AudioEngine {
        AudioEngine { shared: Arc::new(Mutex::new(Mixer::new(OFFLINE_RATE))), sink: None, tried: false, device_error: None, scratch: Vec::new(), offline: true }
    }

    /// Opens the sound card now (once). Returns whether sound will be heard.
    pub fn open_device(&mut self) -> bool {
        if self.tried || self.sink.is_some() {
            return self.sink.is_some();
        }
        self.tried = true;
        match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut sink) => {
                sink.log_on_drop(false);
                let rate = sink.config().sample_rate();
                // The mixer is rebuilt at the card's rate; nothing is playing yet (this is the first sound).
                self.shared = Arc::new(Mutex::new(Mixer::new(rate.get())));
                sink.mixer().add(MixerSource { shared: self.shared.clone(), buf: Vec::new(), pos: 0, rate });
                self.sink = Some(sink);
                true
            }
            Err(e) => {
                self.device_error = Some(format!("no sound card: {e}"));
                false
            }
        }
    }

    /// Call before the first sound of a session is started: opens the sound card unless it is switched off.
    pub fn prepare(&mut self) {
        if !self.offline {
            self.open_device();
        }
    }

    pub fn has_device(&self) -> bool {
        self.sink.is_some()
    }

    pub fn mixer(&self) -> MutexGuard<'_, Mixer> {
        self.shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Lets `dt` seconds pass. With a sound card the card keeps time; without one the mix is rendered into a buffer that is
    /// thrown away, so sounds advance and end all the same.
    pub fn update(&mut self, dt: f32) {
        if self.sink.is_some() {
            return;
        }
        let mut mixer = self.shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if mixer.voice_count() == 0 {
            return;
        }
        let mut frames = (dt.clamp(0.0, 0.5) * mixer.rate() as f32) as usize;
        self.scratch.resize(BLOCK_FRAMES * 2, 0.0);
        while frames > 0 {
            let n = frames.min(BLOCK_FRAMES);
            mixer.render(&mut self.scratch[..n * 2]);
            frames -= n;
        }
    }
}

/// The mix as a rodio source (stereo, forever).
struct MixerSource {
    shared: Arc<Mutex<Mixer>>,
    buf: Vec<f32>,
    pos: usize,
    rate: NonZero<u32>,
}

impl Iterator for MixerSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.buf.len() {
            self.buf.resize(BLOCK_FRAMES * 2, 0.0);
            match self.shared.lock() {
                Ok(mut mixer) => mixer.render(&mut self.buf),
                Err(poisoned) => poisoned.into_inner().render(&mut self.buf),
            }
            self.pos = 0;
        }
        let v = self.buf[self.pos];
        self.pos += 1;
        Some(v)
    }
}

impl Source for MixerSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(2).expect("two is not zero")
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{
        clip::Clip,
        mixer::{MixerEvent, VoiceParams},
    };

    #[test]
    fn without_a_sound_card_time_still_passes_and_sounds_end() {
        let mut engine = AudioEngine::offline();
        let id = {
            let mut m = engine.mixer();
            let rate = m.rate();
            m.play(Arc::new(Clip::from_mono(rate, &vec![0.3; rate as usize / 2])), 0.0, VoiceParams::default(), &[])
        };
        engine.update(0.25);
        assert!(engine.mixer().is_playing(id));
        assert!((engine.mixer().position(id).unwrap() - 0.25).abs() < 0.01);
        engine.update(0.3);
        assert!(!engine.mixer().is_playing(id));
        assert_eq!(engine.mixer().take_events(), vec![MixerEvent::Ended(id)]);
        assert!(!engine.has_device());
        engine.prepare();
        assert!(!engine.has_device(), "an offline engine never opens the card");
    }

    #[test]
    fn a_frame_that_took_very_long_does_not_make_the_mixer_render_for_minutes() {
        let mut engine = AudioEngine::offline();
        let id = engine.mixer().play(Arc::new(Clip::from_mono(48_000, &vec![0.3; 48_000 * 10])), 0.0, VoiceParams::default(), &[]);
        engine.update(60.0);
        assert!(engine.mixer().position(id).unwrap() <= 0.51, "at most half a second passes per update");
    }
}
