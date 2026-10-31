//! TremoloSoundEffect: the volume wobbles.

use super::sound_effect::PARENT;
use crate::{audio::EffectSpec, core::*, dm::DataModel};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Depth", 1.0).group("Tremolo").order(0).slider(0.0, 1.0).desc("How deep the wobble goes (1: down to silence)."),
    PropMeta::number("Duty", 0.5).group("Tremolo").order(1).slider(0.05, 0.95).desc("The share of each wobble spent getting quieter."),
    PropMeta::number("Frequency", 5.0).group("Tremolo").order(2).slider(0.01, 40.0).desc("Wobbles per second."),
];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    Some(EffectSpec::Tremolo { depth: dm.num(id, "Depth", 1.0), frequency: dm.num(id, "Frequency", 5.0), duty: dm.num(id, "Duty", 0.5) })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("TremoloEffect").describe("Makes the volume of the sounds it is in wobble.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("TremoloSoundEffect", Some("SoundEffect"))
    .describe("The volume wobbles.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["Tremolo"])
    .components(&["TremoloEffect"])
    .props(PROPS);