//! ColorCorrectionEffect: brightness, contrast, saturation and a tint, applied to the picture of every camera as Bevy's colour
//! grading (`render::camera_fx`): brightness is exposure, saturation -1 is grey, a tint warms or cools the whole picture.

use crate::{
    core::*,
    dm::DataModel,
    render::camera_fx::{CameraEffects, ColorFx},
};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Brightness", 0.0).group("Colour").order(0).flags(F_VISUAL).slider(-1.0, 1.0).desc("Makes the picture lighter or darker."),
    PropMeta::number("Contrast", 0.0).group("Colour").order(1).flags(F_VISUAL).slider(-1.0, 1.0).desc("Pushes light and dark apart."),
    PropMeta::number("Saturation", 0.0).group("Colour").order(2).flags(F_VISUAL).slider(-1.0, 1.0).desc("-1 is grey, 1 is vivid."),
    PropMeta::color3("TintColor", 1.0, 1.0, 1.0).group("Colour").order(3).flags(F_VISUAL).desc("Tints the whole picture (a warm colour makes it warmer)."),
];

fn camera_effect(dm: &DataModel, id: Id, fx: &mut CameraEffects) {
    if dm.flag(id, "Enabled") {
        fx.color = Some(ColorFx { brightness: dm.num(id, "Brightness", 0.0), contrast: dm.num(id, "Contrast", 0.0), saturation: dm.num(id, "Saturation", 0.0), tint: dm.vec3(id, "TintColor").to_array() });
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("ColorCorrectionFx").describe("Grades the colours of every camera.").camera_effect(camera_effect);

pub static CLASS: ClassDesc = ClassDesc::new("ColorCorrectionEffect", Some("PostEffect"))
    .describe("Adjusts the colours of the picture.")
    .category("Lighting")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Lighting"))
    .groups(&["Colour"])
    .components(&["CameraEffect", "ColorCorrectionFx"])
    .props(PROPS);