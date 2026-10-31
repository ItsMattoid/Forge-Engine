//! CompressorSoundEffect: evens out the volume.

use super::sound_effect::PARENT;
use crate::{audio::EffectSpec, core::*, dm::DataModel};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Threshold", -40.0).group("Compressor").order(0).slider(-80.0, 0.0).desc("The level, in decibels, above which the sound is turned down."),
    PropMeta::number("Ratio", 40.0).group("Compressor").order(1).slider(1.0, 100.0).desc("How hard it is turned down: at 4, every 4 decibels over the threshold become 1."),
    PropMeta::number("Attack", 0.1).group("Compressor").order(2).slider(0.0005, 2.0).desc("Seconds it takes to start turning a loud sound down."),
    PropMeta::number("Release", 0.1).group("Compressor").order(3).slider(0.0005, 5.0).desc("Seconds it takes to let go once the sound is quiet again."),
    PropMeta::number("GainMakeup", 0.0).group("Compressor").order(4).slider(-30.0, 30.0).desc("Decibels added to the whole sound afterwards."),
];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    Some(EffectSpec::Compressor {
        threshold_db: dm.num(id, "Threshold", -40.0),
        ratio: dm.num(id, "Ratio", 40.0),
        attack: dm.num(id, "Attack", 0.1),
        release: dm.num(id, "Release", 0.1),
        makeup_db: dm.num(id, "GainMakeup", 0.0),
    })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("CompressorEffect").describe("Evens out the volume of the sounds it is in.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("CompressorSoundEffect", Some("SoundEffect"))
    .describe("Evens out the volume.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["Compressor"])
    .components(&["CompressorEffect"])
    .props(PROPS);