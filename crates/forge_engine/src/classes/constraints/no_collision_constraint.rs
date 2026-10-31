//! NoCollisionConstraint: stops two parts from colliding with each other (they still collide with everything else).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("NoCollisionConstraint", Some("Constraint"))
    .describe("Stops two parts from colliding with each other.")
    .category("Constraints")
    .creatable();