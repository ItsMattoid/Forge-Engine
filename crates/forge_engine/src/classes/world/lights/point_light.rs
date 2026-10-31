//! PointLight: shines in every direction from a point.
//!
//! Everything specific to this light type is in this file: its `Range` property,
//! the class descriptor, and `PointLightEmitter`, the component that creates and
//! updates the Bevy `PointLight`. Colour, intensity, shadows and the on/off switch
//! come from `Light`.

use bevy::prelude::*;

use super::light::{self, Common};
use crate::{
    compat,
    core::{ClassDesc, ComponentDesc, ParentRule, PropMeta},
    dm::{DataModel, Id},
    render::RenderCtx,
};

static PROPS: &[PropMeta] = &[light::RANGE];

// ---- rendering ----------------------------------------------------------------------------------

fn apply(ctx: &mut RenderCtx, dm: &DataModel, id: Id, entity: Entity, common: &Common) {
    let bevy_light = PointLight {
        color: common.color,
        intensity: common.intensity * light::LUMENS_PER_UNIT,
        range: dm.num(id, "Range", 16.0),
        shadow_maps_enabled: common.shadows,
        ..default()
    };
    compat::insert(ctx.commands, entity, bevy_light);
}

fn render(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, bits: u8) {
    light::sync(ctx, dm, id, bits, apply);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("PointLightEmitter")
    .describe("Emits light in every direction from the object's position.")
    .requires(&["Transform"])
    .render(render)
    .gizmo(light::range_gizmo);

// ---- class --------------------------------------------------------------------------------------

pub static CLASS: ClassDesc = ClassDesc::new("PointLight", Some("Light"))
    .describe("A light that shines in every direction.")
    .category("Lights")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Workspace"))
    .components(&["PointLightEmitter"])
    .props(PROPS);
