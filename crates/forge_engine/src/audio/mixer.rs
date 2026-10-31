//! The mixer: plays any number of [`Clip`]s at once into one stereo stream.
//!
//! A [`Voice`] is one sound being played: where it is in its clip, how loud and how fast, whether it loops, how it is
//! placed between the ears, and its chain of effects. The mixer reads each voice with linear interpolation (so a
//! different `speed`, or a clip of another sample rate, is a change of pitch), smooths volume changes over a few
//! milliseconds (no clicks), runs the effects, adds everything up and keeps the sum from clipping.
//!
//! The mixer does not know about sound devices: [`Mixer::render`] fills a buffer, whoever owns the buffer decides where it
//! goes (the sound card in `engine`, a test in the tests). Time passes only when it renders, so a game with no sound card
//! can still advance its sounds by rendering into a scratch buffer, and `Ended` arrives when it should.

use std::sync::Arc;

use super::{
    clip::Clip,
    dsp::{Effect, EffectSpec},
};

/// How many sounds can play at once. A new one past this replaces the quietest.
pub const MAX_VOICES: usize = 64;

/// Names a playing sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VoiceId(pub u64);

/// How a voice is played. The mixer applies changes smoothly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceParams {
    /// Loudness, 0 up (1 is the file as it is).
    pub volume: f32,
    /// Playback speed (and so pitch): 1 is normal, 2 an octave up.
    pub speed: f32,
    pub looped: bool,
    /// Where between the ears: -1 left, 0 middle, 1 right (equal power).
    pub pan: f32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        VoiceParams { volume: 1.0, speed: 1.0, looped: false, pan: 0.0 }
    }
}

/// What happened to a voice since the last [`Mixer::take_events`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerEvent {
    /// A voice that does not loop played to its end (and is gone).
    Ended(VoiceId),
    /// A looping voice went back to the start.
    Looped(VoiceId),
}

struct Voice {
    id: VoiceId,
    clip: Arc<Clip>,
    /// Position in the clip, in frames.
    pos: f64,
    params: VoiceParams,
    paused: bool,
    /// The gains actually applied, moving toward the wanted ones.
    gain: (f32, f32),
    effects: Vec<(EffectSpec, Box<dyn Effect>)>,
    /// Samples of fade-in left (a sound that starts at once would click).
    fade_in: u32,
}

impl Voice {
    fn wanted_gain(&self) -> (f32, f32) {
        // Equal-power pan: the total energy stays the same as the sound moves.
        let angle = (self.params.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let v = self.params.volume.max(0.0);
        (v * angle.cos(), v * angle.sin())
    }
}

pub struct Mixer {
    rate: u32,
    voices: Vec<Voice>,
    next_id: u64,
    events: Vec<MixerEvent>,
    /// Multiplies the whole mix.
    pub master: f32,
}

/// How fast a gain follows its target (per sample): about 5 ms.
const GAIN_SMOOTHING: f32 = 0.004;

impl Mixer {
    pub fn new(rate: u32) -> Mixer {
        Mixer { rate: rate.max(8000), voices: Vec::new(), next_id: 1, events: Vec::new(), master: 1.0 }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Starts a clip `start` seconds in. `effects` are the chain it plays through.
    pub fn play(&mut self, clip: Arc<Clip>, start: f32, params: VoiceParams, effects: &[EffectSpec]) -> VoiceId {
        if self.voices.len() >= MAX_VOICES {
            // The quietest goes.
            if let Some(quietest) = self.voices.iter().enumerate().min_by(|a, b| a.1.params.volume.total_cmp(&b.1.params.volume)).map(|(i, _)| i) {
                self.voices.remove(quietest);
            }
        }
        let id = VoiceId(self.next_id);
        self.next_id += 1;
        let pos = (f64::from(start.max(0.0)) * f64::from(clip.rate)).min(clip.frames() as f64);
        let mut voice = Voice { id, clip, pos, params, paused: false, gain: (0.0, 0.0), effects: Vec::new(), fade_in: (self.rate / 400).max(8) };
        voice.gain = voice.wanted_gain();
        voice.effects = effects.iter().map(|s| (*s, s.build(self.rate))).collect();
        self.voices.push(voice);
        id
    }

    fn voice_mut(&mut self, id: VoiceId) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|v| v.id == id)
    }

    pub fn is_playing(&self, id: VoiceId) -> bool {
        self.voices.iter().any(|v| v.id == id)
    }

    pub fn params(&self, id: VoiceId) -> Option<VoiceParams> {
        self.voices.iter().find(|v| v.id == id).map(|v| v.params)
    }

    pub fn effect_count(&self, id: VoiceId) -> Option<usize> {
        self.voices.iter().find(|v| v.id == id).map(|v| v.effects.len())
    }

    pub fn set_params(&mut self, id: VoiceId, params: VoiceParams) {
        if let Some(v) = self.voice_mut(id) {
            v.params = params;
        }
    }

    pub fn set_paused(&mut self, id: VoiceId, paused: bool) {
        if let Some(v) = self.voice_mut(id) {
            v.paused = paused;
        }
    }

    /// Replaces the effect chain. Effects that are the same kind in the same place keep their running state.
    pub fn set_effects(&mut self, id: VoiceId, specs: &[EffectSpec]) {
        let rate = self.rate;
        let Some(v) = self.voice_mut(id) else { return };
        if v.effects.len() == specs.len() && v.effects.iter().zip(specs).all(|((old, _), new)| old == new) {
            return;
        }
        let mut old = std::mem::take(&mut v.effects);
        v.effects = Vec::with_capacity(specs.len());
        for (i, spec) in specs.iter().enumerate() {
            // The effect that was there keeps running if it can take the new numbers.
            let kept = old.get_mut(i).and_then(|(_, fx)| fx.update(spec).then(|| std::mem::replace(fx, Box::new(Silence))));
            v.effects.push((*spec, kept.unwrap_or_else(|| spec.build(rate))));
        }
    }

    /// Jumps to `seconds` into the clip.
    pub fn seek(&mut self, id: VoiceId, seconds: f32) {
        if let Some(v) = self.voice_mut(id) {
            v.pos = (f64::from(seconds.max(0.0)) * f64::from(v.clip.rate)).min(v.clip.frames() as f64);
        }
    }

    /// Where the voice is, in seconds of the clip.
    pub fn position(&self, id: VoiceId) -> Option<f32> {
        self.voices.iter().find(|v| v.id == id).map(|v| (v.pos / f64::from(v.clip.rate)) as f32)
    }

    pub fn stop(&mut self, id: VoiceId) {
        self.voices.retain(|v| v.id != id);
    }

    pub fn stop_all(&mut self) {
        self.voices.clear();
    }

    pub fn take_events(&mut self) -> Vec<MixerEvent> {
        std::mem::take(&mut self.events)
    }

    /// Mixes `out.len() / 2` frames of interleaved stereo, overwriting `out`.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let rate = f64::from(self.rate);
        let mut finished: Vec<VoiceId> = Vec::new();
        for voice in &mut self.voices {
            if voice.paused {
                continue;
            }
            let clip = voice.clip.clone();
            let frames = clip.frames();
            let step = f64::from(voice.params.speed.clamp(0.05, 8.0)) * f64::from(clip.rate) / rate;
            let (target_l, target_r) = voice.wanted_gain();
            let mut ended = false;
            for frame in out.chunks_exact_mut(2) {
                if voice.pos >= frames as f64 {
                    if voice.params.looped && frames > 0 {
                        voice.pos -= frames as f64;
                        self.events.push(MixerEvent::Looped(voice.id));
                    } else {
                        ended = true;
                        break;
                    }
                }
                let i = voice.pos as usize;
                let frac = (voice.pos - i as f64) as f32;
                let (l0, r0) = clip.frame(i);
                // (Past the last frame a looping sound blends toward its first, so the seam does not click.)
                let (l1, r1) = if i + 1 < frames { clip.frame(i + 1) } else if voice.params.looped { clip.frame(0) } else { (0.0, 0.0) };
                let (mut l, mut r) = (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac);
                voice.pos += step;
                for (_, fx) in &mut voice.effects {
                    (l, r) = fx.process(l, r);
                }
                voice.gain.0 += (target_l - voice.gain.0) * GAIN_SMOOTHING;
                voice.gain.1 += (target_r - voice.gain.1) * GAIN_SMOOTHING;
                let fade = if voice.fade_in > 0 {
                    voice.fade_in -= 1;
                    1.0 - voice.fade_in as f32 / (self.rate / 400).max(8) as f32
                } else {
                    1.0
                };
                frame[0] += l * voice.gain.0 * fade;
                frame[1] += r * voice.gain.1 * fade;
            }
            if ended || (!voice.params.looped && voice.pos >= frames as f64) {
                finished.push(voice.id);
            }
        }
        for id in finished {
            self.voices.retain(|v| v.id != id);
            self.events.push(MixerEvent::Ended(id));
        }
        let master = self.master;
        for s in out.iter_mut() {
            *s = soft_clip(*s * master);
        }
    }
}

/// Leaves quiet sound alone and bends loud sound toward ±1 instead of cutting it off.
fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        x.signum() * (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh())
    }
}

/// What an effect slot holds while its effect has been moved.
struct Silence;

impl Effect for Silence {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (l, r)
    }

    fn update(&mut self, _spec: &EffectSpec) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48000;

    fn tone(hz: f32, seconds: f32) -> Arc<Clip> {
        let n = (RATE as f32 * seconds) as usize;
        Arc::new(Clip::from_mono(RATE, &(0..n).map(|i| (i as f32 / RATE as f32 * hz * std::f32::consts::TAU).sin() * 0.5).collect::<Vec<_>>()))
    }

    fn render(m: &mut Mixer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        m.render(&mut out);
        out
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| f32::max(m, v.abs()))
    }

    #[test]
    fn a_sound_plays_to_its_end_once_and_is_then_gone() {
        let mut m = Mixer::new(RATE);
        let id = m.play(tone(440.0, 0.1), 0.0, VoiceParams::default(), &[]);
        let first = render(&mut m, 2400);
        assert!(peak(&first) > 0.2, "it is heard");
        assert!(m.is_playing(id));
        assert!((m.position(id).unwrap() - 0.05).abs() < 0.002, "{:?}", m.position(id));
        let rest = render(&mut m, 4000);
        assert!(!m.is_playing(id), "gone at its end");
        assert_eq!(m.take_events(), vec![MixerEvent::Ended(id)]);
        assert!(peak(&rest[4800..]) < 1e-6, "and silent after it");
        assert!(m.take_events().is_empty(), "the end is reported once");
    }

    #[test]
    fn a_looped_sound_goes_round_and_reports_each_lap() {
        let mut m = Mixer::new(RATE);
        let id = m.play(tone(440.0, 0.05), 0.0, VoiceParams { looped: true, ..VoiceParams::default() }, &[]);
        let out = render(&mut m, 12_000);
        assert!(m.is_playing(id));
        assert!(peak(&out[20_000..]) > 0.2, "still sounding after several laps");
        let laps = m.take_events().iter().filter(|e| **e == MixerEvent::Looped(id)).count();
        assert!((4..=6).contains(&laps), "0.25 s of a 0.05 s sound is five laps, got {laps}");
    }

    #[test]
    fn speed_changes_the_length_and_the_pitch() {
        let length = |speed: f32| {
            let mut m = Mixer::new(RATE);
            let id = m.play(tone(440.0, 0.2), 0.0, VoiceParams { speed, ..VoiceParams::default() }, &[]);
            let mut frames = 0;
            while m.is_playing(id) && frames < 100_000 {
                render(&mut m, 100);
                frames += 100;
            }
            frames
        };
        let (normal, double, half) = (length(1.0), length(2.0), length(0.5));
        assert!((normal as f32 - 9600.0).abs() < 200.0, "{normal}");
        assert!((double as f32 - normal as f32 / 2.0).abs() < 200.0, "twice as fast is half as long: {double}");
        assert!((half as f32 - normal as f32 * 2.0).abs() < 300.0, "{half}");
        // Pitch: count the waves in a second of a looped tone.
        let waves = |speed: f32| {
            let mut m = Mixer::new(RATE);
            m.play(tone(200.0, 1.0), 0.0, VoiceParams { speed, looped: true, ..VoiceParams::default() }, &[]);
            let out = render(&mut m, RATE as usize / 2);
            out.chunks(2).map(|f| f[0]).collect::<Vec<_>>().windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
        };
        assert!((waves(2.0) as f32 / waves(1.0) as f32 - 2.0).abs() < 0.1);
    }

    #[test]
    fn volume_scales_and_panning_places_the_sound_between_the_ears_with_constant_power() {
        let level = |volume: f32, pan: f32| {
            let mut m = Mixer::new(RATE);
            m.play(tone(300.0, 0.3), 0.0, VoiceParams { volume, pan, ..VoiceParams::default() }, &[]);
            let out = render(&mut m, 8000);
            let side = |k: usize| peak(&out[4000..].iter().skip(k).step_by(2).copied().collect::<Vec<_>>());
            (side(0), side(1))
        };
        let (l, r) = level(1.0, 0.0);
        assert!((l - r).abs() < 0.01 && l > 0.3, "centred: {l} {r}");
        let (l2, _) = level(0.5, 0.0);
        assert!((l2 / l - 0.5).abs() < 0.03, "half the volume");
        let (l, r) = level(1.0, -1.0);
        assert!(l > 0.4 && r < 0.01, "hard left: {l} {r}");
        let (l, r) = level(1.0, 1.0);
        assert!(r > 0.4 && l < 0.01, "hard right: {l} {r}");
        let (cl, cr) = level(1.0, 0.0);
        let (hl, hr) = level(1.0, 0.5);
        assert!(((cl * cl + cr * cr) - (hl * hl + hr * hr)).abs() < 0.03, "the power does not change as it pans");
        let (z, _) = level(0.0, 0.0);
        assert!(z < 1e-3, "volume 0 is silence");
    }

    #[test]
    fn a_change_of_volume_is_smoothed_not_stepped() {
        let mut m = Mixer::new(RATE);
        let id = m.play(Arc::new(Clip::from_mono(RATE, &vec![0.5; 40_000])), 0.0, VoiceParams::default(), &[]);
        render(&mut m, 2000);
        m.set_params(id, VoiceParams { volume: 0.0, ..VoiceParams::default() });
        let out = render(&mut m, 2000);
        let biggest_jump = out.chunks(2).map(|f| f[0]).collect::<Vec<_>>().windows(2).fold(0f32, |m, w| m.max((w[1] - w[0]).abs()));
        assert!(biggest_jump < 0.02, "no click: {biggest_jump}");
        assert!(peak(&out[3000..]) < 0.01, "and it does get quiet");
    }

    #[test]
    fn pause_holds_the_position_and_seek_moves_it() {
        let mut m = Mixer::new(RATE);
        let id = m.play(tone(440.0, 1.0), 0.0, VoiceParams::default(), &[]);
        render(&mut m, 4800);
        m.set_paused(id, true);
        let held = m.position(id).unwrap();
        let out = render(&mut m, 4800);
        assert_eq!(m.position(id).unwrap(), held, "time stands still");
        assert!(peak(&out) == 0.0, "and so does the sound");
        m.set_paused(id, false);
        m.seek(id, 0.5);
        assert!((m.position(id).unwrap() - 0.5).abs() < 1e-3);
        render(&mut m, 4800);
        assert!((m.position(id).unwrap() - 0.6).abs() < 0.002);
        m.seek(id, 99.0);
        render(&mut m, 100);
        assert!(!m.is_playing(id), "seeking past the end ends it");
    }

    #[test]
    fn starting_in_the_middle_of_a_clip_and_stopping_work() {
        let mut m = Mixer::new(RATE);
        let id = m.play(tone(440.0, 1.0), 0.75, VoiceParams::default(), &[]);
        assert!((m.position(id).unwrap() - 0.75).abs() < 1e-3);
        m.stop(id);
        assert!(!m.is_playing(id) && m.voice_count() == 0);
        assert!(m.take_events().is_empty(), "a sound that was stopped did not end");
        m.stop(id); // (stopping what is gone is fine)
    }

    #[test]
    fn effects_are_part_of_the_voice_and_can_be_changed_while_it_plays() {
        let mut m = Mixer::new(RATE);
        let id = m.play(tone(300.0, 1.0), 0.0, VoiceParams::default(), &[]);
        render(&mut m, 3000);
        let dry = peak(&render(&mut m, 3000));
        m.set_effects(id, &[EffectSpec::Filter { kind: super::super::dsp::FilterType::Low, cutoff: 50.0, resonance: 0.7 }]);
        render(&mut m, 6000);
        let filtered = peak(&render(&mut m, 3000));
        assert!(filtered < dry * 0.3, "a 50 Hz low pass takes most of a 300 Hz tone: {dry} -> {filtered}");
        m.set_effects(id, &[]);
        render(&mut m, 3000);
        assert!(peak(&render(&mut m, 3000)) > dry * 0.8, "without the effect it is back");
    }

    #[test]
    fn the_mix_never_clips_however_many_loud_sounds_play() {
        let mut m = Mixer::new(RATE);
        for _ in 0..40 {
            m.play(Arc::new(Clip::from_mono(RATE, &vec![0.9; 4800])), 0.0, VoiceParams::default(), &[]);
        }
        assert_eq!(m.voice_count(), 40);
        let out = render(&mut m, 4000);
        assert!(peak(&out) <= 1.0, "{}", peak(&out));
        assert!(peak(&out) > 0.95, "but it is loud");
        assert!((soft_clip(0.3) - 0.3).abs() < 1e-9, "quiet sound is untouched");
        assert!(soft_clip(100.0) <= 1.0 && soft_clip(-100.0) >= -1.0);
    }

    #[test]
    fn past_the_voice_limit_the_quietest_is_replaced() {
        let mut m = Mixer::new(RATE);
        let clip = tone(300.0, 1.0);
        let quiet = m.play(clip.clone(), 0.0, VoiceParams { volume: 0.01, ..VoiceParams::default() }, &[]);
        for _ in 0..MAX_VOICES - 1 {
            m.play(clip.clone(), 0.0, VoiceParams::default(), &[]);
        }
        assert_eq!(m.voice_count(), MAX_VOICES);
        m.play(clip, 0.0, VoiceParams::default(), &[]);
        assert_eq!(m.voice_count(), MAX_VOICES);
        assert!(!m.is_playing(quiet));
    }

    #[test]
    fn a_clip_of_another_sample_rate_plays_at_the_right_speed() {
        let mut m = Mixer::new(RATE);
        let clip = Arc::new(Clip::from_mono(24_000, &vec![0.4; 24_000]));
        let id = m.play(clip, 0.0, VoiceParams::default(), &[]);
        render(&mut m, RATE as usize / 2);
        assert!((m.position(id).unwrap() - 0.5).abs() < 0.002, "half a second of the clip after half a second of the mix");
    }
}
