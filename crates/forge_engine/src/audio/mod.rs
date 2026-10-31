//! Sound: decoding, mixing, effects, 3D placement, and the mirror that plays the data model's `Sound` objects.
//!
//! ```text
//! clip     a decoded sound file                          dsp      effects (reverb, echo, filters ...)
//! mixer    plays clips together into a stereo stream     spatial  distance, panning and Doppler for 3D sounds
//! engine   the mixer + the sound card (or none)          sync     Sound instances -> voices, and back
//! ```
//!
//! Like physics, audio is a mirror of the data model: a `Sound` says what it wants (`Playing`, `Volume`, `PlaybackSpeed`,
//! `Looped`, `TimePosition`, where it sits, the effects inside it), and `sync` makes the mixer do that, then writes back what
//! only the mixer knows (where the sound is, that it ended). Nothing in the data model, the scripting host or the editor
//! knows about the mixer: classes opt in through the `SoundEmitter` component and the `audio_effect` hook of the
//! sound-effect classes.

pub mod clip;
pub mod dsp;
pub mod engine;
pub mod mixer;
pub mod spatial;
pub mod sync;

pub use clip::Clip;
pub use dsp::EffectSpec;
pub use engine::AudioEngine;
pub use mixer::{Mixer, MixerEvent, VoiceId, VoiceParams};
pub use sync::SoundMirror;

use crate::{core::Id, dm::DataModel};

/// What a sound-effect class says about itself: the settings of the processor it stands for (`None`: this one does nothing).
pub type EffectFn = fn(&DataModel, Id) -> Option<EffectSpec>;