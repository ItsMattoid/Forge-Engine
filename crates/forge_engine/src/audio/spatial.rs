//! 3D sound: how far a sound is heard, which ear it is nearer, and how its pitch shifts as it moves (Doppler).
//!
//! Pure arithmetic on positions, so it is tested without a mixer. `audio::sync` feeds it the listener (the current camera)
//! and the emitter (the part or attachment the `Sound` is in).

use bevy::math::{Quat, Vec3};

/// How the volume of a sound falls with distance (`Sound.RollOffMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollOff {
    /// `min / distance`: loud up close, a long thin tail.
    Inverse,
    /// Straight down to nothing at the maximum distance.
    Linear,
    /// The straight line squared: falls fast at first.
    LinearSquare,
    /// `Inverse` that is pulled down to nothing at the maximum distance.
    InverseTapered,
}

impl RollOff {
    pub fn from_name(name: &str) -> RollOff {
        match name {
            "Linear" => RollOff::Linear,
            "LinearSquare" => RollOff::LinearSquare,
            "InverseTapered" => RollOff::InverseTapered,
            _ => RollOff::Inverse,
        }
    }
}

/// The share of the volume that reaches the listener `distance` studs away: 1 up to `min`, nothing from `max`.
pub fn attenuation(mode: RollOff, distance: f32, min: f32, max: f32) -> f32 {
    let (min, max) = (min.max(0.0), max.max(min.max(0.0)));
    if !distance.is_finite() || distance >= max {
        return 0.0;
    }
    if distance <= min || max <= min {
        return 1.0;
    }
    let along = (distance - min) / (max - min);
    let inverse = min.max(1e-3) / distance;
    // (Even `Inverse` is faded over the last tenth of the range, so a sound does not cut off at the limit.)
    let edge = 1.0 - ((along - 0.9) / 0.1).clamp(0.0, 1.0);
    let edge = edge * edge * (3.0 - 2.0 * edge);
    match mode {
        RollOff::Inverse => inverse * edge,
        RollOff::Linear => 1.0 - along,
        RollOff::LinearSquare => (1.0 - along) * (1.0 - along),
        RollOff::InverseTapered => inverse * (1.0 - along),
    }
}

/// Where the listener is and how it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    pub pos: Vec3,
    /// Looks along -Z, has +X on its right.
    pub rot: Quat,
    pub velocity: Vec3,
}

impl Default for Listener {
    fn default() -> Self {
        Listener { pos: Vec3::ZERO, rot: Quat::IDENTITY, velocity: Vec3::ZERO }
    }
}

/// How a positioned sound reaches the listener.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Distance attenuation.
    pub gain: f32,
    /// -1 left to 1 right.
    pub pan: f32,
    /// Playback-rate factor from the Doppler effect (1: none).
    pub doppler: f32,
    pub distance: f32,
}

/// The speed of sound in metres per second.
pub const SPEED_OF_SOUND: f32 = 343.0;

#[derive(Clone, Debug, PartialEq)]
pub struct SpatialSettings {
    pub mode: RollOff,
    pub min_distance: f32,
    pub max_distance: f32,
    /// `SoundService.RolloffScale`: multiplies distances (2 makes every sound fade twice as fast).
    pub rolloff_scale: f32,
    /// `SoundService.DopplerScale`: 0 turns the effect off.
    pub doppler_scale: f32,
    /// `SoundService.DistanceFactor`: studs per metre, to turn the speed of sound into studs per second.
    pub studs_per_metre: f32,
}

impl Default for SpatialSettings {
    fn default() -> Self {
        SpatialSettings { mode: RollOff::Inverse, min_distance: 10.0, max_distance: 10_000.0, rolloff_scale: 1.0, doppler_scale: 1.0, studs_per_metre: 3.33 }
    }
}

/// Where a sound at `emitter` (moving at `emitter_velocity`) is for `listener`.
pub fn place(listener: &Listener, emitter: Vec3, emitter_velocity: Vec3, s: &SpatialSettings) -> Placement {
    let to_emitter = emitter - listener.pos;
    let distance = to_emitter.length();
    let gain = attenuation(s.mode, distance * s.rolloff_scale.max(0.0), s.min_distance, s.max_distance);
    let local = listener.rot.inverse() * to_emitter;
    // Right of the listener is +X. Straight ahead or behind (or on top of the listener) is the middle.
    let pan = if distance < 1e-3 { 0.0 } else { (local.x / distance).clamp(-1.0, 1.0) };
    let doppler = if distance < 1e-3 || s.doppler_scale <= 0.0 {
        1.0
    } else {
        let dir = to_emitter / distance;
        let c = SPEED_OF_SOUND * s.studs_per_metre.max(0.01);
        // Positive: the listener moves toward the emitter / the emitter moves away from the listener.
        let (listener_speed, emitter_speed) = (listener.velocity.dot(dir), emitter_velocity.dot(dir));
        let shift = (c + listener_speed) / (c + emitter_speed).max(c * 0.1);
        (1.0 + (shift - 1.0) * s.doppler_scale).clamp(0.5, 2.0)
    };
    Placement { gain, pan, doppler, distance }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_roll_off_is_full_up_close_and_silent_at_the_limit() {
        for mode in [RollOff::Inverse, RollOff::Linear, RollOff::LinearSquare, RollOff::InverseTapered] {
            assert_eq!(attenuation(mode, 0.0, 10.0, 100.0), 1.0, "{mode:?}");
            assert_eq!(attenuation(mode, 10.0, 10.0, 100.0), 1.0, "{mode:?} at the minimum");
            assert_eq!(attenuation(mode, 100.0, 10.0, 100.0), 0.0, "{mode:?} at the maximum");
            assert_eq!(attenuation(mode, 5000.0, 10.0, 100.0), 0.0);
            let mut last = 1.0;
            for d in (10..100).step_by(5) {
                let g = attenuation(mode, d as f32, 10.0, 100.0);
                assert!(g <= last + 1e-6 && (0.0..=1.0).contains(&g), "{mode:?} gets quieter with distance: {d} -> {g}");
                last = g;
            }
        }
    }

    #[test]
    fn the_curves_have_the_shapes_their_names_promise() {
        assert!((attenuation(RollOff::Inverse, 20.0, 10.0, 1000.0) - 0.5).abs() < 1e-4, "twice as far, half as loud");
        assert!((attenuation(RollOff::Inverse, 40.0, 10.0, 1000.0) - 0.25).abs() < 1e-4);
        assert!((attenuation(RollOff::Linear, 55.0, 10.0, 100.0) - 0.5).abs() < 1e-4);
        assert!((attenuation(RollOff::LinearSquare, 55.0, 10.0, 100.0) - 0.25).abs() < 1e-4);
        assert!(attenuation(RollOff::InverseTapered, 55.0, 10.0, 100.0) < attenuation(RollOff::Inverse, 55.0, 10.0, 100.0));
        assert_eq!(RollOff::from_name("LinearSquare"), RollOff::LinearSquare);
        assert_eq!(RollOff::from_name("anything else"), RollOff::Inverse);
    }

    #[test]
    fn nonsense_distances_are_silence_or_full_never_nan() {
        assert_eq!(attenuation(RollOff::Inverse, f32::NAN, 10.0, 100.0), 0.0);
        assert_eq!(attenuation(RollOff::Linear, 5.0, 50.0, 10.0), 1.0, "a maximum below the minimum is the minimum");
        assert_eq!(attenuation(RollOff::Linear, 5.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn a_sound_on_the_right_pans_right_and_one_ahead_or_behind_stays_in_the_middle() {
        let l = Listener::default();
        let s = SpatialSettings::default();
        assert!(place(&l, Vec3::new(8.0, 0.0, 0.0), Vec3::ZERO, &s).pan > 0.99);
        assert!(place(&l, Vec3::new(-8.0, 0.0, 0.0), Vec3::ZERO, &s).pan < -0.99);
        assert!(place(&l, Vec3::new(0.0, 0.0, -8.0), Vec3::ZERO, &s).pan.abs() < 1e-5, "ahead");
        assert!(place(&l, Vec3::new(0.0, 0.0, 8.0), Vec3::ZERO, &s).pan.abs() < 1e-5, "behind");
        assert_eq!(place(&l, Vec3::ZERO, Vec3::ZERO, &s).pan, 0.0, "on top of the listener");
        // Turn the listener to the right: what was on the right is now behind it, what was ahead is on its left.
        let turned = Listener { rot: Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2), ..l };
        assert!(place(&turned, Vec3::new(0.0, 0.0, -8.0), Vec3::ZERO, &s).pan < -0.99, "ahead of the old view is on the left now");
    }

    #[test]
    fn a_far_sound_is_quieter_and_the_rolloff_scale_makes_everything_fade_faster() {
        let l = Listener::default();
        let near = place(&l, Vec3::new(0.0, 0.0, -10.0), Vec3::ZERO, &SpatialSettings::default());
        let far = place(&l, Vec3::new(0.0, 0.0, -100.0), Vec3::ZERO, &SpatialSettings::default());
        assert!(near.gain == 1.0 && far.gain < 0.2);
        assert_eq!(far.distance, 100.0);
        let steep = place(&l, Vec3::new(0.0, 0.0, -50.0), Vec3::ZERO, &SpatialSettings { rolloff_scale: 2.0, ..SpatialSettings::default() });
        let normal = place(&l, Vec3::new(0.0, 0.0, -50.0), Vec3::ZERO, &SpatialSettings::default());
        assert!(steep.gain < normal.gain * 0.6);
    }

    #[test]
    fn the_doppler_effect_raises_the_pitch_of_what_comes_closer_and_lowers_what_goes_away() {
        let l = Listener::default();
        let s = SpatialSettings::default();
        let coming = place(&l, Vec3::new(0.0, 0.0, -200.0), Vec3::new(0.0, 0.0, 300.0), &s).doppler;
        let going = place(&l, Vec3::new(0.0, 0.0, -200.0), Vec3::new(0.0, 0.0, -300.0), &s).doppler;
        let still = place(&l, Vec3::new(0.0, 0.0, -200.0), Vec3::ZERO, &s).doppler;
        assert!(coming > 1.1 && going < 0.9 && still == 1.0, "{coming} {going} {still}");
        let sideways = place(&l, Vec3::new(200.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 300.0), &s).doppler;
        assert!((sideways - 1.0).abs() < 1e-4, "moving across the line of sight does not shift the pitch");
        let off = place(&l, Vec3::new(0.0, 0.0, -200.0), Vec3::new(0.0, 0.0, 300.0), &SpatialSettings { doppler_scale: 0.0, ..SpatialSettings::default() });
        assert_eq!(off.doppler, 1.0);
        let me_moving = place(&Listener { velocity: Vec3::new(0.0, 0.0, -300.0), ..l }, Vec3::new(0.0, 0.0, -200.0), Vec3::ZERO, &s).doppler;
        assert!(me_moving > 1.1, "walking toward a sound raises it too");
        let extreme = place(&l, Vec3::new(0.0, 0.0, -200.0), Vec3::new(0.0, 0.0, 1.0e6), &s).doppler;
        assert!((0.5..=2.0).contains(&extreme));
    }
}
