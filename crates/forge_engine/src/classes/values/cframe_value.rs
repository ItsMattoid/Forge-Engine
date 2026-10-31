//! CFrameValue: holds one CFrame that scripts can read, change and watch with `Changed`. (It is not saved with the scene: scene files do not hold CFrames.)

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::cframe("Value").group("Value").no_save().desc("The stored value.")];

pub static CLASS: ClassDesc = ClassDesc::new("CFrameValue", Some("ValueBase"))
    .describe("Holds a CFrame.")
    .category("Values")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .groups(&["Value"])
    .components(&["ValueObject"])
    .props(PROPS);