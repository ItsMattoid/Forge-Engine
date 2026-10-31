//! AreaLight: a rectangular light source.
//!
//! Bevy has no area lights, so `AreaLightEmitter` approximates one with a soft
//! spherical point light about the size of the panel. This file holds the panel
//! size, the class descriptor and that approximation.

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
    PropMeta::number("Width", 4.0).group("Area").order(0).flags(F_VISUAL).drag(0.1).clamp(0.01, 1000.0).desc("Width of the light panel."),
    PropMeta::number("Height", 4.0).group("Area").order(1).flags(F_VISUAL).drag(0.1).clamp(0.01, 1000.0).desc("Height of the light panel."),
];

// ---- rendering ----------------------------------------------------------------------------------

fn apply(ctx: &mut RenderCtx, dm: &DataModel, id: Id, entity: Entity, common: &Common) {
    let radius = dm.num(id, "Width", 4.0).max(dm.num(id, "Height", 4.0)) * 0.5;
    let bevy_light = PointLight {
        color: common.color,
        intensity: common.intensity * light::LUMENS_PER_UNIT,
        range: dm.num(id, "Range", 16.0),
        radius,
        shadow_maps_enabled: common.shadows,
        ..default()
    };
    compat::insert(ctx.commands, entity, bevy_light);
}

fn render(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, bits: u8) {
    light::sync(ctx, dm, id, bits, apply);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("AreaLightEmitter")
    .describe("Emits light from a rectangular panel (approximated by a soft point light).")
    .requires(&["Transform"])
    .render(render)
    .gizmo(light::range_gizmo);

// ---- class --------------------------------------------------------------------------------------

pub static CLASS: ClassDesc = ClassDesc::new("AreaLight", Some("Light"))
    .describe("A rectangular light panel.")
    .category("Lights")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Workspace"))
    .groups(&["Area"])
    .components(&["AreaLightEmitter"])
    .props(PROPS);
