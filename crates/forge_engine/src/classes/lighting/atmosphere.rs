//! Atmosphere: haze and the colour of the air, drawn as distance fog (`render::camera_fx`). `Density` and `Haze` say how far one
//! can see, `Color` is the colour of the air near, `Decay` the colour it takes from the light far away, and `Glare` lights the
//! haze up around the sun. (`Offset`, how much haze is kept at the horizon, is kept for scripts: fog has no horizon.)

use crate::{
    core::*,
    dm::DataModel,
    render::camera_fx::{CameraEffects, FogFx, FogKind},
};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Density", 0.395).group("Atmosphere").order(0).flags(F_VISUAL).slider(0.0, 1.0).desc("How thick the air is: the thicker, the shorter the distance one can see."),
    PropMeta::number("Offset", 0.0).group("Atmosphere").order(1).flags(F_VISUAL).slider(0.0, 1.0).hidden().desc("How much of the haze is kept at the horizon (kept for scripts)."),
    PropMeta::color3("Color", 0.78, 0.667, 0.42).group("Atmosphere").order(2).flags(F_VISUAL).desc("The colour of the air."),
    PropMeta::color3("Decay", 0.36, 0.235, 0.05).group("Atmosphere").order(3).flags(F_VISUAL).desc("The colour of the air far away (what it takes out of the light)."),
    PropMeta::number("Glare", 0.0).group("Atmosphere").order(4).flags(F_VISUAL).slider(0.0, 10.0).desc("How much light the haze scatters around the sun."),
    PropMeta::number("Haze", 0.0).group("Atmosphere").order(5).flags(F_VISUAL).slider(0.0, 10.0).desc("How hazy the air is: more shortens the distance one can see."),
];

/// How far one can see through this air, in studs.
pub fn visibility(density: f32, haze: f32) -> f32 {
    (4000.0 * (1.0 - density.clamp(0.0, 1.0)).powi(3) + 120.0) / (1.0 + 0.4 * haze.max(0.0))
}

fn camera_effect(dm: &DataModel, id: Id, fx: &mut CameraEffects) {
    fx.atmosphere = Some(FogFx {
        color: dm.vec3(id, "Color").to_array(),
        kind: FogKind::Atmospheric { visibility: visibility(dm.num(id, "Density", 0.395), dm.num(id, "Haze", 0.0)), extinction: dm.vec3(id, "Decay").to_array() },
        glare: dm.num(id, "Glare", 0.0),
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("AtmosphereFx").describe("Fills the world with haze for every camera.").camera_effect(camera_effect);

pub static CLASS: ClassDesc = ClassDesc::new("Atmosphere", Some("Instance"))
    .describe("Haze and the colour of the air.")
    .category("Lighting")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Lighting"))
    .groups(&["Atmosphere"])
    .components(&["CameraEffect", "AtmosphereFx"])
    .props(PROPS);