//! BloomEffect: bright things glow. Drawn by Bevy's bloom on every camera (`render::camera_fx`): `Intensity` is how strong the
//! glow is, `Size` how far it spreads and `Threshold` how bright a surface must be to glow. What glows is what is brighter than
//! the screen can show: Neon and any material with emission, the sun on shiny things, bright lights.

use crate::{
    core::*,
    dm::DataModel,
    render::camera_fx::{BloomFx, CameraEffects},
};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Intensity", 1.0).group("Bloom").order(0).flags(F_VISUAL).drag(0.05).clamp(0.0, 100.0).desc("How strong the glow is."),
    PropMeta::number("Size", 24.0).group("Bloom").order(1).flags(F_VISUAL).drag(0.5).clamp(0.0, 56.0).desc("How far the glow spreads."),
    PropMeta::number("Threshold", 2.0).group("Bloom").order(2).flags(F_VISUAL).drag(0.05).clamp(0.0, 100.0).desc("How bright a surface must be to glow."),
];

fn camera_effect(dm: &DataModel, id: Id, fx: &mut CameraEffects) {
    if dm.flag(id, "Enabled") {
        fx.bloom = Some(BloomFx { intensity: dm.num(id, "Intensity", 1.0), size: dm.num(id, "Size", 24.0), threshold: dm.num(id, "Threshold", 2.0) });
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("BloomFx").describe("Makes bright things glow on every camera.").camera_effect(camera_effect);

pub static CLASS: ClassDesc = ClassDesc::new("BloomEffect", Some("PostEffect"))
    .describe("Makes bright things glow.")
    .category("Lighting")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Lighting"))
    .groups(&["Bloom"])
    .components(&["CameraEffect", "BloomFx"])
    .props(PROPS);