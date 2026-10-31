//! FilterSoundEffect: lets the lows, the highs or a band through and cuts the rest.

use super::sound_effect::PARENT;
use crate::{
    audio::{dsp::FilterType, EffectSpec},
    core::*,
    dm::DataModel,
};

pub static ENUMS: &[EnumDesc] = &[EnumDesc::new("SoundFilterType", &["LowPass", "HighPass", "BandPass"])];

static PROPS: &[PropMeta] = &[
    PropMeta::enumeration("Type", "SoundFilterType", "LowPass").group("Filter").order(0).desc("LowPass cuts the highs (muffled), HighPass cuts the lows (thin), BandPass keeps only a band around the cutoff."),
    PropMeta::number("Cutoff", 1000.0).group("Filter").order(1).drag(10.0).clamp(20.0, 20000.0).desc("The frequency, in hertz, where the sound starts to be cut."),
    PropMeta::number("Resonance", 1.0).group("Filter").order(2).slider(0.3, 20.0).desc("How much the sound near the cutoff is emphasised."),
];

fn spec(dm: &DataModel, id: Id) -> Option<EffectSpec> {
    let kind = match dm.str_ref(id, "Type") {
        "HighPass" => FilterType::High,
        "BandPass" => FilterType::Band,
        _ => FilterType::Low,
    };
    Some(EffectSpec::Filter { kind, cutoff: dm.num(id, "Cutoff", 1000.0), resonance: dm.num(id, "Resonance", 1.0) })
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("FilterEffect").describe("Cuts frequencies from the sounds it is in.").audio_effect(spec);

pub static CLASS: ClassDesc = ClassDesc::new("FilterSoundEffect", Some("SoundEffect"))
    .describe("Lets the lows, the highs or a band through and cuts the rest.")
    .category("Audio")
    .creatable()
    .parent_rule(PARENT)
    .groups(&["Filter"])
    .components(&["FilterEffect"])
    .props(PROPS);