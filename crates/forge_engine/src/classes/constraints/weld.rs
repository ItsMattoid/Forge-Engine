//! Weld: holds two parts so that `Part0 * C0` is where `Part1 * C1` is - the parts are put into that relation and kept
//! there. (`WeldConstraint` keeps the parts as they are; a Weld has frames.)

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("Weld", Some("JointInstance"))
    .describe("Holds two parts together at the places its frames say.")
    .category("Constraints")
    .creatable()
    .components(&["JointFrames"]);