//! SoundEffect: the base of the audio processors that sit inside a `Sound` or a `SoundGroup` (`ReverbSoundEffect`,
//! `EchoSoundEffect` ...). An effect changes how the sounds it is in are heard; the effects of a sound run in `Priority`
//! order (lowest first), then those of its sound groups. The processors themselves are in `audio::dsp`; each subclass says
//! which one it stands for with the `audio_effect` hook of its component.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Effect").order(0).desc("Whether the effect is applied."),
    PropMeta::integer("Priority", 0).group("Effect").order(1).clamp(-1000.0, 1000.0).desc("The order effects are applied in: the lowest first."),
];

pub static CLASS: ClassDesc = ClassDesc::new("SoundEffect", Some("Instance"))
    .describe("Base class of the audio processors inside a Sound or SoundGroup.")
    .groups(&["Effect"])
    .props(PROPS);

/// How a subclass is inserted: inside the selected Sound or SoundGroup.
pub const PARENT: ParentRule = ParentRule::Selected { class: "Instance", hint: "Select a Sound or a SoundGroup first: an effect changes the sound it is in." };