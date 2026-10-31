//! CornerWedgePart: a part shaped like a pyramid corner (a `Part` whose `Shape` is CornerWedge).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("CornerWedgePart", Some("Part"))
    .describe("A part shaped like a corner of a pyramid.")
    .category("Parts")
    .creatable()
    .spawn_at_focus()
    .insert_props(&[("Anchored", D::B(true))])
    .hide(&["Shape"])
    .defaults(&[("Shape", D::E("CornerWedge"))]);