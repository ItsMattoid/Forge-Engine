//! DepthOfFieldEffect: blurs what is not in focus (Bevy's depth of field, `render::camera_fx`). `FocusDistance` is what is sharp,
//! `InFocusRadius` how far around it stays sharp, `NearIntensity` / `FarIntensity` how blurred the rest is.

use crate::{
    core::*,
    dm::DataModel,
    render::camera_fx::{CameraEffects, DofFx},
};

static PROPS: &[PropMeta] = &[
    PropMeta::number("FarIntensity", 0.75).group("Depth of field").order(0).flags(F_VISUAL).slider(0.0, 1.0).desc("How blurred things beyond the focus are."),
    PropMeta::number("NearIntensity", 0.75).group("Depth of field").order(1).flags(F_VISUAL).slider(0.0, 1.0).desc("How blurred things before the focus are."),
    PropMeta::number("FocusDistance", 0.05).group("Depth of field").order(2).flags(F_VISUAL).drag(0.5).clamp(0.0, 1.0e5).desc("The distance of what is in focus."),
    PropMeta::number("InFocusRadius", 10.0).group("Depth of field").order(3).flags(F_VISUAL).drag(0.5).clamp(0.0, 1.0e5).desc("How far from the focus things stay sharp."),
];

fn camera_effect(dm: &DataModel, id: Id, fx: &mut CameraEffects) {
    if dm.flag(id, "Enabled") {
        fx.dof = Some(DofFx { focus_distance: dm.num(id, "FocusDistance", 0.05), in_focus_radius: dm.num(id, "InFocusRadius", 10.0), near: dm.num(id, "NearIntensity", 0.75), far: dm.num(id, "FarIntensity", 0.75) });
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("DepthOfFieldFx").describe("Blurs what is out of focus on every camera.").camera_effect(camera_effect);

pub static CLASS: ClassDesc = ClassDesc::new("DepthOfFieldEffect", Some("PostEffect"))
    .describe("Blurs what is not in focus.")
    .category("Lighting")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Lighting"))
    .groups(&["Depth of field"])
    .components(&["CameraEffect", "DepthOfFieldFx"])
    .props(PROPS);