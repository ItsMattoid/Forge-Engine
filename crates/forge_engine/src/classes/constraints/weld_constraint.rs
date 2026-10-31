//! WeldConstraint: holds two parts together exactly as they are when the weld is made (there is no frame to set).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("WeldConstraint", Some("Constraint")).describe("Holds two parts rigidly together, as they are placed.").category("Constraints").creatable();
