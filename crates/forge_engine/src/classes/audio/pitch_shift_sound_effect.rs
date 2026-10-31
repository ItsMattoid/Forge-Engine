//! PitchShiftSoundEffect: higher or lower, at the same speed.

use super::sound_effect::PARENT;
use crate::{audio::EffectSpec, core::*, dm::DataModel};

static PROPS: &[PropMeta] = &[PropMeta::number("Octave", 1.25).group("PitchShift").order(0).slider(0.25, 4.0).desc("How many times as high: 2 is an octave up, 0.5 an octave down, 1 no change. (PlaybackSpeed changes the pitch and the length together.)")];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    Some(EffectSpec::PitchShift { octave: dm.num(id, "Octave", 1.25), mix: 1.0 })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("PitchShiftEffect").describe("Changes the pitch of the sounds it is in without changing their speed.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("PitchShiftSoundEffect", Some("SoundEffect"))
    .describe("Higher or lower, at the same speed.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["PitchShift"])
    .components(&["PitchShiftEffect"])
    .props(PROPS);