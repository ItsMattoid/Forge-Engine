//! ChorusSoundEffect: the sound doubled with slightly detuned copies of itself.

use super::sound_effect::PARENT;
use crate::{audio::EffectSpec, core::*, dm::DataModel};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Depth", 0.45).group("Chorus").order(0).slider(0.0, 1.0).desc("How far the copies drift in pitch."),
    PropMeta::number("Mix", 0.5).group("Chorus").order(1).slider(0.0, 1.0).desc("How much of the copies is heard."),
    PropMeta::number("Rate", 0.5).group("Chorus").order(2).slider(0.01, 20.0).desc("How fast the drift swings, in hertz."),
];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    Some(EffectSpec::Chorus { depth: dm.num(id, "Depth", 0.45), mix: dm.num(id, "Mix", 0.5), rate: dm.num(id, "Rate", 0.5) })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("ChorusEffect").describe("Doubles the sounds it is in with detuned copies.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("ChorusSoundEffect", Some("SoundEffect"))
    .describe("The sound doubled with detuned copies of itself.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["Chorus"])
    .components(&["ChorusEffect"])
    .props(PROPS);