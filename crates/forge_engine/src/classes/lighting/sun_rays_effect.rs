//! SunRaysEffect: beams of light from the sun, as volumetric light shafts (Bevy's volumetric fog, `render::camera_fx`): a thin
//! fog fills the world and the sun's light shows in it where it is not blocked, so the sun needs shadows (`Lighting.GlobalShadows`).
//! `Intensity` is how thick the fog is, `Spread` how much the light is scattered toward the camera.

use crate::{
    core::*,
    dm::DataModel,
    render::camera_fx::{CameraEffects, SunRaysFx},
};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Intensity", 0.25).group("Sun rays").order(0).flags(F_VISUAL).slider(0.0, 1.0).desc("How bright the rays are."),
    PropMeta::number("Spread", 1.0).group("Sun rays").order(1).flags(F_VISUAL).slider(0.0, 1.0).desc("How far the rays spread."),
];

fn camera_effect(dm: &DataModel, id: Id, fx: &mut CameraEffects) {
    if dm.flag(id, "Enabled") {
        fx.sun_rays = Some(SunRaysFx { intensity: dm.num(id, "Intensity", 0.25), spread: dm.num(id, "Spread", 1.0) });
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("SunRaysFx").describe("Shows the sun's light shafts on every camera.").camera_effect(camera_effect);

pub static CLASS: ClassDesc = ClassDesc::new("SunRaysEffect", Some("PostEffect"))
    .describe("Beams of light from the sun.")
    .category("Lighting")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Lighting"))
    .groups(&["Sun rays"])
    .components(&["CameraEffect", "SunRaysFx"])
    .props(PROPS);