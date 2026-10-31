//! SpotLight: a cone of light along the object's forward direction.
//!
//! This file holds the cone angles, the class descriptor, and `SpotLightEmitter`,
//! the component that creates and updates the Bevy `SpotLight`.

use bevy::prelude::*;

use super::light::{self, Common};
use crate::{
    compat,
    core::{ClassDesc, ComponentDesc, ParentRule, PropMeta, F_VISUAL},
    dm::{DataModel, Id},
    render::RenderCtx,
};

static PROPS: &[PropMeta] = &[
    light::RANGE,
    PropMeta::number("OuterAngle", 90.0)
        .label("Angle")
        .group("Spotlight")
        .order(0)
        .flags(F_VISUAL)
        .slider(1.0, 179.0)
        .aliases(&["Angle"])
        .desc("Full angle of the whole cone, in degrees."),
    PropMeta::number("InnerAngle", 45.0).group("Spotlight").order(1).flags(F_VISUAL).slider(1.0, 179.0).desc("Full angle of the fully lit centre of the cone, in degrees."),
    PropMeta::enumeration("Face", "NormalId", "Front").group("Spotlight").order(2).flags(F_VISUAL).desc("Which side of the part (or which way from the light) the cone points; Front is the light's forward direction."),
];

// ---- rendering ----------------------------------------------------------------------------------

fn apply(ctx: &mut RenderCtx, dm: &DataModel, id: Id, entity: Entity, common: &Common) {
    // The properties are full angles; Bevy wants the half angle from the axis.
    let outer = (dm.num(id, "OuterAngle", 90.0) * 0.5).to_radians().clamp(0.01, std::f32::consts::FRAC_PI_2 - 0.01);
    let inner = (dm.num(id, "InnerAngle", 45.0) * 0.5).to_radians().clamp(0.0, outer);
    let bevy_light = SpotLight {
        color: common.color,
        intensity: common.intensity * light::LUMENS_PER_UNIT,
        range: dm.num(id, "Range", 16.0),
        shadow_maps_enabled: common.shadows,
        outer_angle: outer,
        inner_angle: inner,
        ..default()
    };
    compat::insert(ctx.commands, entity, bevy_light);
}

fn render(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, bits: u8) {
    light::sync(ctx, dm, id, bits, apply);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("SpotLightEmitter")
    .describe("Emits a cone of light along the object's forward direction.")
    .requires(&["Transform"])
    .render(render)
    .gizmo(light::cone_gizmo);

// ---- class --------------------------------------------------------------------------------------

pub static CLASS: ClassDesc = ClassDesc::new("SpotLight", Some("Light"))
    .describe("A cone of light.")
    .category("Lights")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Workspace"))
    .groups(&["Spotlight"])
    .components(&["SpotLightEmitter"])
    .props(PROPS);
