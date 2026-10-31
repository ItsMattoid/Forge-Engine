//! Sky: the picture behind the world, from six pictures of a cube (`render::camera_fx`). The sky also lights the world: shiny
//! surfaces reflect it and shadowed ones take its colour. A scene without a Sky has the plain sky colour of `Lighting`. (The sun,
//! moon and stars Roblox draws are kept as settings for scripts; Forge does not draw them.)

use crate::{
    core::*,
    dm::DataModel,
    render::camera_fx::{CameraEffects, SkyFx},
};

static PROPS: &[PropMeta] = &[
    PropMeta::asset("SkyboxBk", "png").group("Sky").order(0).flags(F_VISUAL).desc("The picture of the back (+Z)."),
    PropMeta::asset("SkyboxDn", "png").group("Sky").order(1).flags(F_VISUAL).desc("The picture of the bottom."),
    PropMeta::asset("SkyboxFt", "png").group("Sky").order(2).flags(F_VISUAL).desc("The picture of the front (-Z)."),
    PropMeta::asset("SkyboxLf", "png").group("Sky").order(3).flags(F_VISUAL).desc("The picture of the left (-X)."),
    PropMeta::asset("SkyboxRt", "png").group("Sky").order(4).flags(F_VISUAL).desc("The picture of the right (+X)."),
    PropMeta::asset("SkyboxUp", "png").group("Sky").order(5).flags(F_VISUAL).desc("The picture of the top."),
    PropMeta::vector3("SkyboxOrientation", 0.0, 0.0, 0.0).group("Sky").order(6).flags(F_VISUAL).drag(1.0).desc("Turns the skybox, in degrees."),
    PropMeta::asset("SunTextureId", "png").group("Sky").order(7).flags(F_VISUAL).hidden().desc("The picture of the sun (kept for scripts)."),
    PropMeta::asset("MoonTextureId", "png").group("Sky").order(8).flags(F_VISUAL).hidden().desc("The picture of the moon (kept for scripts)."),
    PropMeta::boolean("CelestialBodiesShown", true).group("Sky").order(9).flags(F_VISUAL).hidden().desc("Whether the sun, moon and stars are shown (kept for scripts)."),
    PropMeta::integer("StarCount", 3000).group("Sky").order(10).flags(F_VISUAL).clamp(0.0, 5000.0).hidden().desc("How many stars there are (kept for scripts)."),
    PropMeta::number("SunAngularSize", 21.0).group("Sky").order(11).flags(F_VISUAL).slider(0.0, 100.0).hidden().desc("How big the sun looks, in degrees (kept for scripts)."),
    PropMeta::number("MoonAngularSize", 11.0).group("Sky").order(12).flags(F_VISUAL).slider(0.0, 100.0).hidden().desc("How big the moon looks, in degrees (kept for scripts)."),
];

fn camera_effect(dm: &DataModel, id: Id, fx: &mut CameraEffects) {
    // Bevy's cube order: +X, -X, +Y, -Y, +Z, -Z.
    let faces = ["SkyboxRt", "SkyboxLf", "SkyboxUp", "SkyboxDn", "SkyboxBk", "SkyboxFt"].map(|p| dm.string(id, p));
    if faces.iter().all(String::is_empty) {
        return;
    }
    fx.sky = Some(SkyFx { faces, orientation: dm.vec3(id, "SkyboxOrientation").to_array() });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("SkyFx").describe("Draws the sky behind the world for every camera.").camera_effect(camera_effect);

pub static CLASS: ClassDesc = ClassDesc::new("Sky", Some("Instance"))
    .describe("The picture behind the world, from six pictures; it also lights the world.")
    .category("Lighting")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Lighting"))
    .groups(&["Sky"])
    .components(&["CameraEffect", "SkyFx"])
    .props(PROPS);