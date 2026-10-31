//! DirectionalLight: parallel light along the object's forward direction, like the
//! sun. Its position does not matter, only its rotation.
//!
//! The `Lighting` service already provides the sun driven by the time of day; add a
//! DirectionalLight for additional fixed lights (a moon, a rim light, ...). It has no
//! `Range`, so it lists none of the range-based properties.

use bevy::prelude::*;

use super::light::{self, Common};
use crate::{
    compat,
    core::{ClassDesc, ComponentDesc, ParentRule},
    dm::{DataModel, Id},
    render::RenderCtx,
};

/// Illuminance in lux per unit of `Intensity` (1.0 is bright daylight).
const LUX_PER_UNIT: f32 = 10_000.0;

// ---- rendering ----------------------------------------------------------------------------------

fn apply(ctx: &mut RenderCtx, _dm: &DataModel, _id: Id, entity: Entity, common: &Common) {
    let bevy_light = DirectionalLight { color: common.color, illuminance: common.intensity * LUX_PER_UNIT, shadow_maps_enabled: common.shadows, ..default() };
    compat::insert(ctx.commands, entity, bevy_light);
}

fn render(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, bits: u8) {
    light::sync(ctx, dm, id, bits, apply);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("DirectionalLightEmitter")
    .describe("Emits parallel light along the object's forward direction.")
    .requires(&["Transform"])
    .render(render)
    .gizmo(light::direction_gizmo);

// ---- class --------------------------------------------------------------------------------------

pub static CLASS: ClassDesc = ClassDesc::new("DirectionalLight", Some("Light"))
    .describe("Parallel light, like the sun.")
    .category("Lights")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Workspace"))
    .components(&["DirectionalLightEmitter"]);
