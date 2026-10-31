//! Part: a block, ball, cylinder, wedge or corner wedge.
//!
//! `PrimitiveRenderer` draws the built-in shape chosen by `Shape` through
//! `BasePart`'s shared renderer. (Ray casts and picking follow the shape too: the
//! `Collider` component reads the `Shape` property. Movement and touches use the whole box.)

use bevy::prelude::*;

use super::base_part::{self, Look};
use crate::{
    core::{ClassDesc, ComponentDesc, EnumDesc, PropMeta, D, F_VISUAL},
    dm::{DataModel, Id},
    render::{Model, RenderCtx},
};

static PROPS: &[PropMeta] = &[PropMeta::enumeration("Shape", "PartType", "Block").group("Geometry").order(0).flags(F_VISUAL).desc("The shape of the part.")];

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("PartType", &["Block", "Ball", "Cylinder", "Wedge", "CornerWedge"], &[1, 0, 2, 3, 4])];

// ---- rendering ----------------------------------------------------------------------------------

fn model(ctx: &mut RenderCtx, dm: &DataModel, id: Id) -> Model {
    ctx.assets.shape_model(dm.str_ref(id, "Shape"))
}

/// Roblox cylinders lie along the part's X axis; Bevy's cylinder mesh stands along Y.
fn transform(dm: &DataModel, id: Id, pos: Vec3, rot: Quat, size: Vec3, native: Vec3) -> Transform {
    if dm.str_ref(id, "Shape") == "Cylinder" {
        Transform { translation: pos, rotation: rot * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), scale: Vec3::new(size.y, size.x, size.z) }
    } else {
        base_part::box_transform(dm, id, pos, rot, size, native)
    }
}

static LOOK: Look = Look { model, transform, studs_uv: true, imported_materials: false };

fn render(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, bits: u8) {
    base_part::sync(ctx, dm, id, bits, &LOOK);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("PrimitiveRenderer")
    .describe("Draws the object as a block, ball or cylinder.")
    .requires(&["Transform", "Appearance"])
    .render(render);

// ---- class --------------------------------------------------------------------------------------

pub static CLASS: ClassDesc = ClassDesc::new("Part", Some("BasePart"))
    .describe("A block, ball, cylinder, wedge or corner wedge.")
    .category("Parts")
    .creatable()
    .spawn_at_focus()
    .insert_props(&[("Anchored", D::B(true))])
    .groups(&["Geometry"])
    .components(&["PrimitiveRenderer"])
    .props(PROPS);
