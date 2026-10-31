//! WedgePart: a part shaped like a ramp (a `Part` whose `Shape` is Wedge, which is how Forge draws and collides it).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("WedgePart", Some("Part"))
    .describe("A part shaped like a ramp.")
    .category("Parts")
    .creatable()
    .spawn_at_focus()
    .insert_props(&[("Anchored", D::B(true))])
    .hide(&["Shape"])
    .defaults(&[("Shape", D::E("Wedge"))]);