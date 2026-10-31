//! MeshPart: a part drawn from a model file in the project (OBJ, glTF / GLB, STL, PLY).
//!
//! It inherits everything from `BasePart` (position, size, colour, material, physics ...). The model is read at its natural
//! size (`MeshSize`) and scaled to fit the part's `Size` (a new MeshPart is given the model's own size, so it appears as made),
//! and collision uses the part's box. `MeshRenderer` loads the file (through the shared asset cache) and draws it with
//! `BasePart`'s shared renderer, with the materials and textures the file brought unless the part names its own.

use super::base_part::{self, Look};
use crate::{
    core::{ClassDesc, ComponentDesc, PropMeta, Value, D, F_VISUAL},
    dm::{DataModel, Id},
    render::{Model, RenderCtx},
};
use bevy::math::Vec3;

/// The natural size of the mesh file: what `Size` is when the part shows the model at the dimensions it was made with.
fn get_mesh_size(dm: &DataModel, id: Id) -> Option<Value> {
    let size = dm.assets().and_then(|a| a.mesh_size(dm.str_ref(id, "MeshId"))).unwrap_or(Vec3::ZERO);
    Some(Value::V3(size))
}

static PROPS: &[PropMeta] = &[
    PropMeta::asset("MeshId", "obj").label("Mesh").group("Geometry").order(0).flags(F_VISUAL).desc("The model file in the project (obj, gltf, glb, stl, ply): drop one here or pick one. A new MeshPart takes the model's own size."),
    PropMeta::vector3("MeshSize", 0.0, 0.0, 0.0).group("Geometry").order(1).no_save().readonly().computed_ro(get_mesh_size).desc("The size of the model in the file, in studs. Size scales the model from this to the part's size."),
    PropMeta::boolean("UseImportedMaterials", true).group("Geometry").order(2).flags(F_VISUAL).desc("Draw each piece of the model with the material the file gives it (OBJ .mtl, glTF materials). Off, or with a Material Asset or Texture set, the part's own material covers the whole model."),
    PropMeta::enumeration("CollisionFidelity", "CollisionFidelity", "Default").group("Geometry").order(3).hidden().desc("How exactly the mesh collides (Forge collides the box)."),
    PropMeta::enumeration("RenderFidelity", "RenderFidelity", "Automatic").group("Geometry").order(4).hidden(),
];

// ---- rendering ----------------------------------------------------------------------------------

fn model(ctx: &mut RenderCtx, dm: &DataModel, id: Id) -> Model {
    ctx.assets.file_model(dm.str_ref(id, "MeshId"), ctx.source)
}

static LOOK: Look = Look { model, transform: base_part::fitted_transform, studs_uv: false, imported_materials: true };

fn render(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, bits: u8) {
    base_part::sync(ctx, dm, id, bits, &LOOK);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("MeshRenderer")
    .describe("Draws the object from a model file.")
    .requires(&["Transform", "Appearance"])
    .render(render);

// ---- class --------------------------------------------------------------------------------------

pub static CLASS: ClassDesc = ClassDesc::new("MeshPart", Some("BasePart"))
    .describe("A part drawn from a model file in the project.")
    .category("Parts")
    .creatable()
    .spawn_at_focus()
    .insert_props(&[("Anchored", D::B(true))])
    .groups(&["Geometry"])
    .components(&["MeshRenderer"])
    .props(PROPS);