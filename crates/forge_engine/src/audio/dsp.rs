//! Sound effects: the audio processors a `Sound` or `SoundGroup` can have as children (`ReverbSoundEffect`,
//! `EchoSoundEffect` ...). Each is described by an [`EffectSpec`] (plain numbers read from the instance) and run by an
//! [`Effect`] (the state: delay lines, filter memory), one chain per playing voice.
//!
//! All of them work on one stereo sample at a time, so a chain is a loop over the voice's effects. They keep their state
//! when the numbers change ([`Effect::update`]), so turning a knob while a sound plays does not click or cut off a reverb
//! tail.
//!
//! Nothing here knows about Roblox or the data model: it is arithmetic on samples, tested without a sound card.

use std::f32::consts::TAU;

/// The dB scale effects use for their dry / wet levels and thresholds.
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// What an effect should do. Every field is a plain number so two specs can be compared and an effect updated in place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectSpec {
    /// A room: `decay` is how many seconds the sound takes to fade by 60 dB.
    Reverb { decay: f32, density: f32, diffusion: f32, dry_db: f32, wet_db: f32 },
    /// Repeats: after `delay` seconds the sound comes back, `feedback` of it again each time.
    Echo { delay: f32, feedback: f32, dry_db: f32, wet_db: f32 },
    /// Clips the sound harder as `level` (0..1) rises.
    Distortion { level: f32 },
    /// A voice or three: the sound mixed with detuned copies of itself.
    Chorus { depth: f32, mix: f32, rate: f32 },
    /// A sweeping comb filter (the "jet plane" sound).
    Flange { depth: f32, mix: f32, rate: f32 },
    /// The volume wobbles `frequency` times a second by `depth` (0..1).
    Tremolo { depth: f32, frequency: f32, duty: f32 },
    /// Three bands: below `low_hz`, between, above `high_hz` (gains in dB).
    Equalizer { low_db: f32, mid_db: f32, high_db: f32, low_hz: f32, high_hz: f32 },
    /// Evens out the volume: above `threshold_db` the level rises `ratio` times slower.
    Compressor { threshold_db: f32, ratio: f32, attack: f32, release: f32, makeup_db: f32 },
    /// Plays the sound `octave` times as high (0.5 an octave down, 2 an octave up) without changing its length.
    PitchShift { octave: f32, mix: f32 },
    /// Cuts the highs (`Low`), the lows (`High`) or both sides (`Band`) around `cutoff` Hz.
    Filter { kind: FilterType, cutoff: f32, resonance: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterType {
    Low,
    High,
    Band,
}

impl EffectSpec {
    /// A name for the kind, to tell whether two specs can share one running effect.
    pub fn kind(&self) -> &'static str {
        match self {
            EffectSpec::Reverb { .. } => "reverb",
            EffectSpec::Echo { .. } => "echo",
            EffectSpec::Distortion { .. } => "distortion",
            EffectSpec::Chorus { .. } => "chorus",
            EffectSpec::Flange { .. } => "flange",
            EffectSpec::Tremolo { .. } => "tremolo",
            EffectSpec::Equalizer { .. } => "equalizer",
            EffectSpec::Compressor { .. } => "compressor",
            EffectSpec::PitchShift { .. } => "pitchshift",
            EffectSpec::Filter { .. } => "filter",
        }
    }

    /// The running effect for this spec at a sample rate.
    pub fn build(&self, rate: u32) -> Box<dyn Effect> {
        let rate = rate.max(8000) as f32;
        match self {
            EffectSpec::Reverb { .. } => Box::new(Reverb::new(rate, self)),
            EffectSpec::Echo { .. } => Box::new(Echo::new(rate, self)),
            EffectSpec::Distortion { .. } => Box::new(Distortion { spec: *self }),
            EffectSpec::Chorus { .. } => Box::new(Modulated::new(rate, self, false)),
            EffectSpec::Flange { .. } => Box::new(Modulated::new(rate, self, true)),
            EffectSpec::Tremolo { .. } => Box::new(Tremolo { rate, phase: 0.0, spec: *self }),
            EffectSpec::Equalizer { .. } => Box::new(Equalizer::new(rate, self)),
            EffectSpec::Compressor { .. } => Box::new(Compressor { rate, env: 0.0, spec: *self }),
            EffectSpec::PitchShift { .. } => Box::new(PitchShift::new(rate, self)),
            EffectSpec::Filter { .. } => Box::new(FilterFx::new(rate, self)),
        }
    }
}

/// A running effect.
pub trait Effect: Send {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32);
    /// New numbers for the same kind of effect; its memory is kept. `false`: the spec is another kind, build a new effect.
    fn update(&mut self, spec: &EffectSpec) -> bool;
}

// ---- building blocks -------------------------------------------------------------------------------------

/// A second-order filter section (the RBJ cookbook).
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn set(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        let inv = 1.0 / a0;
        (self.b0, self.b1, self.b2, self.a1, self.a2) = (b0 * inv, b1 * inv, b2 * inv, a1 * inv, a2 * inv);
    }

    fn low_pass(&mut self, rate: f32, hz: f32, q: f32) {
        let (w, cos) = Self::w(rate, hz);
        let alpha = w.sin() / (2.0 * q);
        self.set((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    }

    fn high_pass(&mut self, rate: f32, hz: f32, q: f32) {
        let (w, cos) = Self::w(rate, hz);
        let alpha = w.sin() / (2.0 * q);
        self.set((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    }

    fn band_pass(&mut self, rate: f32, hz: f32, q: f32) {
        let (w, cos) = Self::w(rate, hz);
        let alpha = w.sin() / (2.0 * q);
        self.set(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    }

    fn peaking(&mut self, rate: f32, hz: f32, q: f32, db: f32) {
        let (w, cos) = Self::w(rate, hz);
        let a = 10f32.powf(db / 40.0);
        let alpha = w.sin() / (2.0 * q);
        self.set(1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a);
    }

    fn low_shelf(&mut self, rate: f32, hz: f32, db: f32) {
        let (w, cos) = Self::w(rate, hz);
        let a = 10f32.powf(db / 40.0);
        let beta = 2.0 * a.sqrt() * w.sin() / 2f32.sqrt();
        self.set(
            a * ((a + 1.0) - (a - 1.0) * cos + beta),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - beta),
            (a + 1.0) + (a - 1.0) * cos + beta,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - beta,
        );
    }

    fn high_shelf(&mut self, rate: f32, hz: f32, db: f32) {
        let (w, cos) = Self::w(rate, hz);
        let a = 10f32.powf(db / 40.0);
        let beta = 2.0 * a.sqrt() * w.sin() / 2f32.sqrt();
        self.set(
            a * ((a + 1.0) + (a - 1.0) * cos + beta),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - beta),
            (a + 1.0) - (a - 1.0) * cos + beta,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - beta,
        );
    }

    fn w(rate: f32, hz: f32) -> (f32, f32) {
        let w = TAU * hz.clamp(10.0, rate * 0.45) / rate;
        (w, w.cos())
    }
}

/// A circular buffer with fractional reads.
#[derive(Clone)]
struct Delay {
    buf: Vec<f32>,
    write: usize,
}

impl Delay {
    fn new(max_samples: usize) -> Delay {
        Delay { buf: vec![0.0; max_samples.max(4)], write: 0 }
    }

    fn push(&mut self, x: f32) {
        self.buf[self.write] = x;
        self.write = (self.write + 1) % self.buf.len();
    }

    /// The sample `back` samples ago (`back` >= 1 reads what was pushed that many pushes ago), interpolated.
    fn read(&self, back: f32) -> f32 {
        let len = self.buf.len();
        let back = back.clamp(1.0, (len - 2) as f32);
        let whole = back.floor();
        let frac = back - whole;
        let at = |k: usize| self.buf[(self.write + len - k) % len];
        let a = at(whole as usize);
        let b = at(whole as usize + 1);
        a + (b - a) * frac
    }
}

// ---- reverb ----------------------------------------------------------------------------------------------

/// Freeverb's delay lengths (at 44.1 kHz).
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.pos] = x + self.store * feedback;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }
}

struct AllPass {
    buf: Vec<f32>,
    pos: usize,
}

impl AllPass {
    fn process(&mut self, x: f32, gain: f32) -> f32 {
        let delayed = self.buf[self.pos];
        let out = delayed - x;
        self.buf[self.pos] = x + delayed * gain;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }
}

struct Reverb {
    rate: f32,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<AllPass>; 2],
    feedback: [f32; 8],
    damp: f32,
    ap_gain: f32,
    dry: f32,
    wet: f32,
}

impl Reverb {
    fn new(rate: f32, spec: &EffectSpec) -> Reverb {
        let scale = rate / 44100.0;
        let size = |n: usize| ((n as f32 * scale) as usize).max(8);
        let side = |extra: usize| -> (Vec<Comb>, Vec<AllPass>) {
            (
                COMBS.iter().map(|n| Comb { buf: vec![0.0; size(n + extra)], pos: 0, store: 0.0 }).collect(),
                ALLPASSES.iter().map(|n| AllPass { buf: vec![0.0; size(n + extra)], pos: 0 }).collect(),
            )
        };
        let ((cl, al), (cr, ar)) = (side(0), side(STEREO_SPREAD));
        let mut r = Reverb { rate, combs: [cl, cr], allpasses: [al, ar], feedback: [0.0; 8], damp: 0.2, ap_gain: 0.5, dry: 1.0, wet: 0.3 };
        r.update(spec);
        r
    }
}

impl Effect for Reverb {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let input = (l + r) * 0.5 * 0.2;
        let mut out = [0.0f32; 2];
        for side in 0..2 {
            let mut sum = 0.0;
            for (i, c) in self.combs[side].iter_mut().enumerate() {
                sum += c.process(input, self.feedback[i], self.damp);
            }
            for a in &mut self.allpasses[side] {
                sum = a.process(sum, self.ap_gain);
            }
            out[side] = sum;
        }
        (l * self.dry + out[0] * self.wet, r * self.dry + out[1] * self.wet)
    }

    fn update(&mut self, spec: &EffectSpec) -> bool {
        let EffectSpec::Reverb { decay, density, diffusion, dry_db, wet_db } = *spec else { return false };
        let decay = decay.clamp(0.1, 20.0);
        for (i, f) in self.feedback.iter_mut().enumerate() {
            // The gain that makes a comb of this length fall 60 dB in `decay` seconds.
            let seconds = self.combs[0][i].buf.len() as f32 / self.rate;
            *f = 10f32.powf(-3.0 * seconds / decay).clamp(0.0, 0.995);
        }
        // Dense rooms are darker (more of the highs soaked up per trip).
        self.damp = (0.5 - density.clamp(0.0, 1.0) * 0.4).clamp(0.05, 0.6);
        self.ap_gain = 0.3 + diffusion.clamp(0.0, 1.0) * 0.3;
        self.dry = db_to_gain(dry_db.clamp(-80.0, 24.0));
        self.wet = db_to_gain(wet_db.clamp(-80.0, 24.0));
        true
    }
}

// ---- echo ------------------------------------------------------------------------------------------------

struct Echo {
    rate: f32,
    left: Delay,
    right: Delay,
    delay: f32,
    feedback: f32,
    dry: f32,
    wet: f32,
}

impl Echo {
    fn new(rate: f32, spec: &EffectSpec) -> Echo {
        let max = (rate * 5.0) as usize + 4;
        let mut e = Echo { rate, left: Delay::new(max), right: Delay::new(max), delay: 0.4, feedback: 0.5, dry: 1.0, wet: 0.5 };
        e.update(spec);
        e
    }
}

impl Effect for Echo {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let back = self.delay * self.rate;
        // Ping-pong: each side feeds the other, so the repeats travel from ear to ear.
        let (dl, dr) = (self.left.read(back), self.right.read(back));
        self.left.push(l + dr * self.feedback);
        self.right.push(r + dl * self.feedback);
        (l * self.dry + dl * self.wet, r * self.dry + dr * self.wet)
    }

    fn update(&mut self, spec: &EffectSpec) -> bool {
        let EffectSpec::Echo { delay, feedback, dry_db, wet_db } = *spec else { return false };
        self.delay = delay.clamp(0.01, 4.9);
        self.feedback = feedback.clamp(0.0, 0.95);
        self.dry = db_to_gain(dry_db.clamp(-80.0, 24.0));
        self.wet = db_to_gain(wet_db.clamp(-80.0, 24.0));
        true
    }
}

// ---- distortion ------------------------------------------------------------------------------------------

struct Distortion {
    spec: EffectSpec,
}

impl Effect for Distortion {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let EffectSpec::Distortion { level } = self.spec else { return (l, r) };
        let drive = 1.0 + level.clamp(0.0, 1.0) * 30.0;
        // tanh saturation, divided by its own gain so a quiet sound stays at about its volume.
        let norm = drive.tanh().max(0.2);
        ((l * drive).tanh() / norm * 0.8, (r * drive).tanh() / norm * 0.8)
    }

    fn update(&mut self, spec: &EffectSpec) -> bool {
        if !matches!(spec, EffectSpec::Distortion { .. }) {
            return false;
        }
        self.spec = *spec;
        true
    }
}

// ---- chorus and flange -----------------------------------------------------------------------------------

/// A delay that a slow oscillator sweeps: long and unfed it is a chorus, short and fed back a flanger.
struct Modulated {
    rate: f32,
    flange: bool,
    left: Delay,
    right: Delay,
    phase: f32,
    depth: f32,
    mix: f32,
    lfo: f32,
}

impl Modulated {
    fn new(rate: f32, spec: &EffectSpec, flange: bool) -> Modulated {
        let max = (rate * 0.06) as usize + 8;
        let mut m = Modulated { rate, flange, left: Delay::new(max), right: Delay::new(max), phase: 0.0, depth: 0.5, mix: 0.5, lfo: 0.5 };
        m.update(spec);
        m
    }
}

impl Effect for Modulated {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (base_ms, swing_ms) = if self.flange { (3.0, 2.5) } else { (22.0, 8.0) };
        // The right side sweeps a quarter turn behind the left, which widens the sound.
        let lfo_l = (self.phase * TAU).sin();
        let lfo_r = ((self.phase + 0.25) * TAU).sin();
        let ms = |lfo: f32| (base_ms + swing_ms * self.depth * lfo) * 0.001 * self.rate;
        let (dl, dr) = (self.left.read(ms(lfo_l)), self.right.read(ms(lfo_r)));
        let feedback = if self.flange { 0.45 } else { 0.0 };
        self.left.push(l + dl * feedback);
        self.right.push(r + dr * feedback);
        self.phase = (self.phase + self.lfo / self.rate).fract();
        let dry = 1.0 - self.mix * 0.5;
        (l * dry + dl * self.mix, r * dry + dr * self.mix)
    }

    fn update(&mut self, spec: &EffectSpec) -> bool {
        match (*spec, self.flange) {
            (EffectSpec::Chorus { depth, mix, rate }, false) | (EffectSpec::Flange { depth, mix, rate }, true) => {
                self.depth = depth.clamp(0.0, 1.0);
                self.mix = mix.clamp(0.0, 1.0);
                self.lfo = rate.clamp(0.01, 20.0);
                true
            }
            _ => false,
        }
    }
}

// ---- tremolo ---------------------------------------------------------------------------------------------

struct Tremolo {
    rate: f32,
    phase: f32,
    spec: EffectSpec,
}

impl Effect for Tremolo {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let EffectSpec::Tremolo { depth, frequency, duty } = self.spec else { return (l, r) };
        // `duty` skews the wave: the share of each cycle the volume spends on its way down.
        let duty = duty.clamp(0.05, 0.95);
        let p = self.phase;
        let warped = if p < duty { 0.5 * p / duty } else { 0.5 + 0.5 * (p - duty) / (1.0 - duty) };
        let g = 1.0 - depth.clamp(0.0, 1.0) * 0.5 * (1.0 - (warped * TAU).cos());
        self.phase = (p + frequency.clamp(0.01, 40.0) / self.rate).fract();
        (l * g, r * g)
    }

    fn update(&mut self, spec: &EffectSpec) -> bool {
        if !matches!(spec, EffectSpec::Tremolo { .. }) {
            return false;
        }
        self.spec = *spec;
        true
    }
}

// ---- equalizer and filter --------------------------------------------------------------------------------

struct Equalizer {
    rate: f32,
    low: [Biquad; 2],
    mid: [Biquad; 2],
    high: [Biquad; 2],
}

impl Equalizer {
    fn new(rate: f32, spec: &EffectSpec) -> Equalizer {
        let mut e = Equalizer { rate, low: [Biquad::default(); 2], mid: [Biquad::default(); 2], high: [Biquad::default(); 2] };
        e.update(spec);
        e
    }
}

impl Effect for Equalizer {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let side = |i: usize, x: f32, s: &mut Equalizer| s.high[i].process(s.mid[i].process(s.low[i].process(x)));
        (side(0, l, self), side(1, r, self))
    }

    fn update(&mut self, spec: &EffectSpec) -> bool {
        let EffectSpec::Equalizer { low_db, mid_db, high_db, low_hz, high_hz } = *spec else { return false };
        let low_hz = low_hz.clamp(20.0, 8000.0);
        let high_hz = high_hz.clamp(low_hz * 1.1, 20000.0);
        let mid_hz = (low_hz * high_hz).sqrt();
        for i in 0..2 {
            self.low[i].low_shelf(self.rate, low_hz, low_db.clamp(-60.0, 24.0));
            self.mid[i].peaking(self.rate, mid_hz, 0.7, mid_db.clamp(-60.0, 24.0));
            self.high[i].high_shelf(self.rate, high_hz, high_db.clamp(-60.0, 24.0));
        }
        true
    }
}

struct FilterFx {
    rate: f32,
    sections: [Biquad; 2],
}

impl FilterFx {
    fn new(rate: f32, spec: &EffectSpec) -> FilterFx {
        let mut f = FilterFx { rate, sections: [Biquad::default(); 2] };
        f.update(spec);
        f
    }
}

impl Effect for FilterFx {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (self.sections[0].process(l), self.sections[1].process(r))
    }

    fn update(&mut self, spec: &EffectSpec) -> bool {
        let EffectSpec::Filter { kind, cutoff, resonance } = *spec else { return false };
        let q = resonance.clamp(0.3, 20.0);
        for s in &mut self.sections {
            match kind {
                FilterType::Low => s.low_pass(self.rate, cutoff, q),
                FilterType::High => s.high_pass(self.rate, cutoff, q),
                FilterType::Band => s.band_pass(self.rate, cutoff, q),
            }
        }
        true
    }
}

// ---- compressor ------------------------------------------------------------------------------------------

struct Compressor {
    rate: f32,
    env: f32,
    spec: EffectSpec,
}

impl Effect for Compressor {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let EffectSpec::Compressor { threshold_db, ratio, attack, release, makeup_db } = self.spec else { return (l, r) };
        let level = l.abs().max(r.abs());
        let coeff = |seconds: f32| (-1.0 / (seconds.max(0.0005) * self.rate)).exp();
        let c = if level > self.env { coeff(attack) } else { coeff(release) };
        self.env = level + (self.env - level) * c;
        let level_db = 20.0 * self.env.max(1e-6).log10();
        let over = level_db - threshold_db;
        let reduction_db = if over > 0.0 { over * (1.0 - 1.0 / ratio.max(1.0)) } else { 0.0 };
        let g = db_to_gain(makeup_db.clamp(-60.0, 60.0) - reduction_db);
        (l * g, r * g)
    }

    fn update(&mut self, spec: &EffectSpec) -> bool {
        if !matches!(spec, EffectSpec::Compressor { .. }) {
            return false;
        }
        self.spec = *spec;
        true
    }
}

// ---- pitch shift -----------------------------------------------------------------------------------------

/// The classic two-tap delay-line pitch shifter: a delay that is swept steadily shortens or lengthens the waves it carries
/// (a Doppler shift), and two taps half a sweep apart, each faded in and out, hide the jump where a tap starts over.
struct PitchShift {
    left: Delay,
    right: Delay,
    window: f32,
    phase: f32,
    ratio: f32,
    mix: f32,
}

impl PitchShift {
    fn new(rate: f32, spec: &EffectSpec) -> PitchShift {
        let window = (rate * 0.06).max(64.0);
        let mut p = PitchShift { left: Delay::new(window as usize * 2 + 8), right: Delay::new(window as usize * 2 + 8), window, phase: 0.0, ratio: 1.0, mix: 1.0 };
        p.update(spec);
        p
    }
}

impl Effect for PitchShift {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.left.push(l);
        self.right.push(r);
        let (mut out_l, mut out_r) = (0.0, 0.0);
        for tap in [self.phase, (self.phase + 0.5).fract()] {
            let back = 1.0 + tap * self.window;
            // (A triangle: the two taps' fades always add up to one.)
            let fade = 1.0 - (2.0 * tap - 1.0).abs();
            out_l += self.left.read(back) * fade;
            out_r += self.right.read(back) * fade;
        }
        // The delay grows when the pitch drops (ratio < 1) and shrinks when it rises.
        self.phase = (self.phase + (1.0 - self.ratio) / self.window).rem_euclid(1.0);
        (l * (1.0 - self.mix) + out_l * self.mix, r * (1.0 - self.mix) + out_r * self.mix)
    }

    fn update(&mut self, spec: &EffectSpec) -> bool {
        let EffectSpec::PitchShift { octave, mix } = *spec else { return false };
        self.ratio = octave.clamp(0.25, 4.0);
        self.mix = mix.clamp(0.0, 1.0);
        true
    }
}

// ---- tests -----------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44100;

    fn run(fx: &mut dyn Effect, input: &[f32]) -> Vec<f32> {
        input.iter().map(|x| fx.process(*x, *x).0).collect()
    }

    fn sine(hz: f32, seconds: f32) -> Vec<f32> {
        (0..(RATE as f32 * seconds) as usize).map(|i| (i as f32 / RATE as f32 * hz * TAU).sin() * 0.5).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// The frequency of a signal, from its zero crossings.
    fn pitch(x: &[f32]) -> f32 {
        let crossings = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 / (x.len() as f32 / RATE as f32)
    }

    #[test]
    fn decibels_are_amplitude_ratios() {
        assert!((db_to_gain(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_gain(-6.0206) - 0.5).abs() < 1e-3);
        assert!((db_to_gain(20.0) - 10.0).abs() < 1e-3);
    }

    #[test]
    fn an_echo_repeats_the_sound_after_its_delay_and_each_repeat_is_quieter() {
        let spec = EffectSpec::Echo { delay: 0.1, feedback: 0.5, dry_db: 0.0, wet_db: 0.0 };
        let mut fx = spec.build(RATE);
        let mut input = vec![0.0; RATE as usize / 2];
        input[0] = 1.0;
        // (Ping-pong: the repeat of the left side arrives on the right first, so look at the sum of both sides.)
        let out: Vec<(f32, f32)> = input.iter().map(|x| fx.process(*x, *x)).collect();
        let at = |s: usize| out[s].0.abs();
        let first = (RATE as f32 * 0.1) as usize;
        assert!(at(first) > 0.9, "the first repeat: {}", at(first));
        assert!(at(first * 2) > 0.4 && at(first * 2) < 0.6, "the second is half: {}", at(first * 2));
        assert!(at(first / 2) < 0.01, "silence in between");
        assert!(out[0].0 > 0.99, "the dry sound comes through at once");
    }

    #[test]
    fn a_reverb_rings_on_after_the_sound_stops_and_dies_away_in_its_decay_time() {
        let spec = EffectSpec::Reverb { decay: 1.5, density: 1.0, diffusion: 1.0, dry_db: -80.0, wet_db: 0.0 };
        let mut fx = spec.build(RATE);
        let mut input = vec![0.0; RATE as usize * 4];
        for v in input.iter_mut().take(2000) {
            *v = 0.8;
        }
        let out = run(fx.as_mut(), &input);
        let early = rms(&out[3000..6000]);
        let mid = rms(&out[RATE as usize..RATE as usize + 3000]);
        let late = rms(&out[RATE as usize * 3..RATE as usize * 3 + 3000]);
        assert!(early > 0.005, "it rings: {early}");
        assert!(mid < early && late < mid, "and fades: {early} {mid} {late}");
        assert!(late < early * 0.1, "by about 60 dB over the decay time ({late} vs {early})");
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn a_longer_decay_rings_longer() {
        let tail = |decay: f32| {
            let mut fx = EffectSpec::Reverb { decay, density: 1.0, diffusion: 1.0, dry_db: -80.0, wet_db: 0.0 }.build(RATE);
            let mut input = vec![0.0; RATE as usize * 3];
            input[..1000].fill(0.8);
            let out = run(fx.as_mut(), &input);
            rms(&out[RATE as usize * 2..])
        };
        assert!(tail(8.0) > tail(0.5) * 3.0);
    }

    #[test]
    fn distortion_flattens_the_peaks_of_a_loud_sound() {
        let mut clean = EffectSpec::Distortion { level: 0.0 }.build(RATE);
        let mut dirty = EffectSpec::Distortion { level: 1.0 }.build(RATE);
        let input = sine(220.0, 0.2);
        let (a, b) = (run(clean.as_mut(), &input), run(dirty.as_mut(), &input));
        // A squashed wave has more energy for the same peak: its crest factor (peak / rms) is lower.
        let crest = |x: &[f32]| x.iter().fold(0f32, |m, v| m.max(v.abs())) / rms(x);
        assert!(crest(&b) < crest(&a) - 0.15, "{} vs {}", crest(&b), crest(&a));
        assert!(b.iter().all(|v| v.abs() <= 1.0));
    }

    #[test]
    fn a_low_pass_filter_keeps_the_lows_and_removes_the_highs() {
        let mut fx = EffectSpec::Filter { kind: FilterType::Low, cutoff: 1000.0, resonance: 0.7 }.build(RATE);
        let low = run(fx.as_mut(), &sine(100.0, 0.3));
        let mut fx = EffectSpec::Filter { kind: FilterType::Low, cutoff: 1000.0, resonance: 0.7 }.build(RATE);
        let high = run(fx.as_mut(), &sine(8000.0, 0.3));
        assert!(rms(&low[4000..]) > 0.3, "{}", rms(&low[4000..]));
        assert!(rms(&high[4000..]) < 0.01, "{}", rms(&high[4000..]));
        let mut hp = EffectSpec::Filter { kind: FilterType::High, cutoff: 1000.0, resonance: 0.7 }.build(RATE);
        assert!(rms(&run(hp.as_mut(), &sine(100.0, 0.3))[4000..]) < 0.01);
    }

    #[test]
    fn the_equalizer_boosts_and_cuts_its_bands() {
        let measure = |db: [f32; 3], hz: f32| {
            let mut fx = EffectSpec::Equalizer { low_db: db[0], mid_db: db[1], high_db: db[2], low_hz: 400.0, high_hz: 4000.0 }.build(RATE);
            let out = run(fx.as_mut(), &sine(hz, 0.4));
            rms(&out[8000..])
        };
        let flat = measure([0.0; 3], 100.0);
        assert!((measure([0.0, 0.0, 0.0], 1000.0) - flat).abs() < 0.01, "flat is flat");
        assert!(measure([12.0, 0.0, 0.0], 100.0) > flat * 3.0, "the lows boosted");
        assert!(measure([-20.0, 0.0, 0.0], 100.0) < flat * 0.2, "the lows cut");
        assert!(measure([0.0, 0.0, -20.0], 12000.0) < measure([0.0; 3], 12000.0) * 0.2, "the highs cut");
        assert!(measure([0.0, 12.0, 0.0], 1265.0) > measure([0.0; 3], 1265.0) * 2.5, "the middle boosted");
    }

    #[test]
    fn a_compressor_brings_a_loud_sound_down_and_leaves_a_quiet_one_alone() {
        let spec = EffectSpec::Compressor { threshold_db: -20.0, ratio: 4.0, attack: 0.001, release: 0.05, makeup_db: 0.0 };
        let loud: Vec<f32> = sine(300.0, 0.4).iter().map(|v| v * 1.8).collect();
        let quiet: Vec<f32> = sine(300.0, 0.4).iter().map(|v| v * 0.02).collect();
        let mut fx = spec.build(RATE);
        let (l, q) = (run(fx.as_mut(), &loud), run(spec.build(RATE).as_mut(), &quiet));
        assert!(rms(&l[8000..]) < rms(&loud[8000..]) * 0.6, "loud: {} -> {}", rms(&loud[8000..]), rms(&l[8000..]));
        assert!((rms(&q[8000..]) - rms(&quiet[8000..])).abs() < rms(&quiet[8000..]) * 0.05, "quiet is left alone");
    }

    #[test]
    fn a_tremolo_makes_the_volume_swing_between_full_and_the_depth() {
        let mut fx = EffectSpec::Tremolo { depth: 1.0, frequency: 5.0, duty: 0.5 }.build(RATE);
        let input = vec![0.5; RATE as usize];
        let out = run(fx.as_mut(), &input);
        let (min, max) = out.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(max > 0.49 && min < 0.02, "{min} {max}");
        let mut none = EffectSpec::Tremolo { depth: 0.0, frequency: 5.0, duty: 0.5 }.build(RATE);
        assert!(run(none.as_mut(), &input).iter().all(|v| (*v - 0.5).abs() < 1e-6));
    }

    #[test]
    fn chorus_and_flange_add_a_moving_copy_and_do_not_blow_up() {
        for spec in [EffectSpec::Chorus { depth: 1.0, mix: 0.8, rate: 1.0 }, EffectSpec::Flange { depth: 1.0, mix: 0.8, rate: 0.5 }] {
            let mut fx = spec.build(RATE);
            let input = sine(440.0, 1.0);
            let out = run(fx.as_mut(), &input);
            assert!(out.iter().all(|v| v.is_finite() && v.abs() < 3.0), "{:?}", spec.kind());
            let diff: f32 = out.iter().zip(&input).map(|(a, b)| (a - b).abs()).sum::<f32>() / input.len() as f32;
            assert!(diff > 0.02, "{} changes the sound ({diff})", spec.kind());
        }
    }

    #[test]
    fn the_pitch_shifter_moves_the_pitch_by_the_ratio() {
        let input = sine(440.0, 1.0);
        for (ratio, expected) in [(2.0, 880.0), (0.5, 220.0)] {
            let mut fx = EffectSpec::PitchShift { octave: ratio, mix: 1.0 }.build(RATE);
            let out = run(fx.as_mut(), &input);
            let p = pitch(&out[10000..]);
            assert!((p - expected).abs() < expected * 0.08, "ratio {ratio}: {p} Hz, wanted {expected}");
        }
        let mut same = EffectSpec::PitchShift { octave: 1.0, mix: 1.0 }.build(RATE);
        assert!((pitch(&run(same.as_mut(), &input)[10000..]) - 440.0).abs() < 15.0);
    }

    #[test]
    fn changing_the_numbers_keeps_the_running_effect_but_another_kind_needs_a_new_one() {
        let mut fx = EffectSpec::Echo { delay: 0.1, feedback: 0.5, dry_db: 0.0, wet_db: 0.0 }.build(RATE);
        assert!(fx.update(&EffectSpec::Echo { delay: 0.2, feedback: 0.3, dry_db: 0.0, wet_db: -6.0 }));
        assert!(!fx.update(&EffectSpec::Distortion { level: 0.5 }));
        let mut rev = EffectSpec::Reverb { decay: 2.0, density: 1.0, diffusion: 1.0, dry_db: 0.0, wet_db: -6.0 }.build(RATE);
        assert!(rev.update(&EffectSpec::Reverb { decay: 4.0, density: 0.5, diffusion: 0.5, dry_db: 0.0, wet_db: -3.0 }));
        assert!(!rev.update(&EffectSpec::Echo { delay: 0.2, feedback: 0.3, dry_db: 0.0, wet_db: 0.0 }));
        assert_eq!(EffectSpec::Flange { depth: 0.0, mix: 0.0, rate: 1.0 }.kind(), "flange");
    }

    #[test]
    fn extreme_numbers_are_kept_in_range_and_never_produce_nan() {
        let wild = [
            EffectSpec::Reverb { decay: -5.0, density: 9.0, diffusion: -3.0, dry_db: 500.0, wet_db: -500.0 },
            EffectSpec::Echo { delay: 99.0, feedback: 7.0, dry_db: 0.0, wet_db: 0.0 },
            EffectSpec::Compressor { threshold_db: 40.0, ratio: 0.0, attack: -1.0, release: 0.0, makeup_db: 0.0 },
            EffectSpec::PitchShift { octave: 100.0, mix: 5.0 },
            EffectSpec::Equalizer { low_db: 99.0, mid_db: -99.0, high_db: 0.0, low_hz: 0.0, high_hz: 0.0 },
            EffectSpec::Filter { kind: FilterType::Band, cutoff: 1.0e9, resonance: 0.0 },
            EffectSpec::Tremolo { depth: 5.0, frequency: 1.0e6, duty: 0.0 },
        ];
        for spec in wild {
            let mut fx = spec.build(RATE);
            let out = run(fx.as_mut(), &sine(300.0, 0.5));
            assert!(out.iter().all(|v| v.is_finite()), "{}", spec.kind());
            assert!(out.iter().all(|v| v.abs() < 1.0e5), "{} stays bounded", spec.kind());
        }
    }
}
