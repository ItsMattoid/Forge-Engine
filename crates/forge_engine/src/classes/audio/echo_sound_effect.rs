//! EchoSoundEffect: the sound comes back after a delay, again and again, fainter each time.

use super::sound_effect::PARENT;
use crate::{audio::EffectSpec, core::*, dm::DataModel};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Delay", 1.0).group("Echo").order(0).slider(0.01, 4.9).desc("Seconds until the first repeat."),
    PropMeta::number("Feedback", 0.5).group("Echo").order(1).slider(0.0, 0.95).desc("How much of each repeat comes back again."),
    PropMeta::number("DryLevel", 0.0).group("Echo").order(2).slider(-80.0, 20.0).desc("How loud the original sound is, in decibels."),
    PropMeta::number("WetLevel", 0.0).group("Echo").order(3).slider(-80.0, 20.0).desc("How loud the repeats are, in decibels."),
];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    Some(EffectSpec::Echo { delay: dm.num(id, "Delay", 1.0), feedback: dm.num(id, "Feedback", 0.5), dry_db: dm.num(id, "DryLevel", 0.0), wet_db: dm.num(id, "WetLevel", 0.0) })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("EchoEffect").describe("Repeats the sounds it is in.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("EchoSoundEffect", Some("SoundEffect"))
    .describe("Repeats the sound, fainter each time.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["Echo"])
    .components(&["EchoEffect"])
    .props(PROPS);