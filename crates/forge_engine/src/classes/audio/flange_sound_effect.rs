//! FlangeSoundEffect: a sweeping, jet-plane whoosh.

use super::sound_effect::PARENT;
use crate::{audio::EffectSpec, core::*, dm::DataModel};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Depth", 0.45).group("Flange").order(0).slider(0.0, 1.0).desc("How far the sweep reaches."),
    PropMeta::number("Mix", 0.85).group("Flange").order(1).slider(0.0, 1.0).desc("How much of the swept copy is heard."),
    PropMeta::number("Rate", 5.0).group("Flange").order(2).slider(0.01, 20.0).desc("How fast it sweeps, in hertz."),
];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    Some(EffectSpec::Flange { depth: dm.num(id, "Depth", 0.45), mix: dm.num(id, "Mix", 0.85), rate: dm.num(id, "Rate", 5.0) })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("FlangeEffect").describe("Sweeps a comb filter over the sounds it is in.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("FlangeSoundEffect", Some("SoundEffect"))
    .describe("A sweeping, jet-plane whoosh.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["Flange"])
    .components(&["FlangeEffect"])
    .props(PROPS);