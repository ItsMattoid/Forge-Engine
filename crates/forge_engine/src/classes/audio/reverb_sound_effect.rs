//! ReverbSoundEffect: the echo of a room.

use super::sound_effect::PARENT;
use crate::{audio::EffectSpec, core::*, dm::DataModel};

static PROPS: &[PropMeta] = &[
    PropMeta::number("DecayTime", 1.5).group("Reverb").order(0).slider(0.1, 20.0).desc("Seconds the sound takes to fade away (a bigger room is longer)."),
    PropMeta::number("Density", 1.0).group("Reverb").order(1).slider(0.0, 1.0).desc("How full the echo is (low: a few bright reflections, high: a smooth, darker wash)."),
    PropMeta::number("Diffusion", 1.0).group("Reverb").order(2).slider(0.0, 1.0).desc("How smooth the echo is."),
    PropMeta::number("DryLevel", -6.0).group("Reverb").order(3).slider(-80.0, 20.0).desc("How loud the original sound is, in decibels."),
    PropMeta::number("WetLevel", 0.0).group("Reverb").order(4).slider(-80.0, 20.0).desc("How loud the echo is, in decibels."),
];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    Some(EffectSpec::Reverb { decay: dm.num(id, "DecayTime", 1.5), density: dm.num(id, "Density", 1.0), diffusion: dm.num(id, "Diffusion", 1.0), dry_db: dm.num(id, "DryLevel", -6.0), wet_db: dm.num(id, "WetLevel", 0.0) })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("ReverbEffect").describe("Adds the echo of a room to the sounds it is in.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("ReverbSoundEffect", Some("SoundEffect"))
    .describe("The echo of a room.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["Reverb"])
    .components(&["ReverbEffect"])
    .props(PROPS);