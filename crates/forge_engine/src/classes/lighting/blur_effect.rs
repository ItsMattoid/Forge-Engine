//! BlurEffect: blurs the picture. It is Bevy's depth of field with everything out of focus (`render::camera_fx`), so what is
//! near the camera is blurred a little less than what is far away; `Size` is how much. A `DepthOfFieldEffect` in the same scene
//! takes over (the two are one effect in the renderer).

use crate::{
    core::*,
    dm::DataModel,
    render::camera_fx::CameraEffects,
};

static PROPS: &[PropMeta] = &[PropMeta::number("Size", 24.0).group("Blur").order(0).flags(F_VISUAL).drag(0.5).clamp(0.0, 56.0).desc("How much the picture is blurred.")];

fn camera_effect(dm: &DataModel, id: Id, fx: &mut CameraEffects) {
    if dm.flag(id, "Enabled") {
        fx.blur = Some(dm.num(id, "Size", 24.0));
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("BlurFx").describe("Blurs the picture of every camera.").camera_effect(camera_effect);

pub static CLASS: ClassDesc = ClassDesc::new("BlurEffect", Some("PostEffect"))
    .describe("Blurs the picture.")
    .category("Lighting")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Lighting"))
    .groups(&["Blur"])
    .components(&["CameraEffect", "BlurFx"])
    .props(PROPS);