//! DistortionSoundEffect: a rough, overdriven sound.

use super::sound_effect::PARENT;
use crate::{audio::EffectSpec, core::*, dm::DataModel};

static PROPS: &[PropMeta] = &[PropMeta::number("Level", 0.5).group("Distortion").order(0).slider(0.0, 1.0).desc("How hard the sound is driven.")];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    Some(EffectSpec::Distortion { level: dm.num(id, "Level", 0.5) })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("DistortionEffect").describe("Overdrives the sounds it is in.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("DistortionSoundEffect", Some("SoundEffect"))
    .describe("A rough, overdriven sound.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["Distortion"])
    .components(&["DistortionEffect"])
    .props(PROPS);