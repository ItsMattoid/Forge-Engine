//! Configuration: a plain container for settings - values and other objects a script reads.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("Configuration", Some("Instance"))
    .describe("A container for settings.")
    .category("Values")
    .creatable()
    .container()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"));