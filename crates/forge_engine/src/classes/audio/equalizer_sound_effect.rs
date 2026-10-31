//! EqualizerSoundEffect: turn the lows, mids and highs up or down.

use super::sound_effect::PARENT;
use crate::{audio::EffectSpec, core::*, dm::DataModel};

static PROPS: &[PropMeta] = &[
    PropMeta::number("LowGain", 0.0).group("Equalizer").order(0).slider(-80.0, 10.0).desc("How much the low sounds (below MidRange) are turned up or down, in decibels."),
    PropMeta::number("MidGain", 0.0).group("Equalizer").order(1).slider(-80.0, 10.0).desc("How much the middle sounds are turned up or down, in decibels."),
    PropMeta::number("HighGain", 0.0).group("Equalizer").order(2).slider(-80.0, 10.0).desc("How much the high sounds (above MidRange) are turned up or down, in decibels."),
    PropMeta::vector2("MidRange", 400.0, 4000.0).group("Equalizer").order(3).desc("Where the middle is: from X to Y hertz."),
];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    let range = match dm.get_prop(id, "MidRange") {
        Some(Value::V2(v)) => v,
        _ => bevy::math::Vec2::new(400.0, 4000.0),
    };
    Some(EffectSpec::Equalizer { low_db: dm.num(id, "LowGain", 0.0), mid_db: dm.num(id, "MidGain", 0.0), high_db: dm.num(id, "HighGain", 0.0), low_hz: range.x, high_hz: range.y })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("EqualizerEffect").describe("Changes the balance of lows, mids and highs of the sounds it is in.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("EqualizerSoundEffect", Some("SoundEffect"))
    .describe("Turns the lows, mids and highs up or down.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["Equalizer"])
    .components(&["EqualizerEffect"])
    .props(PROPS);