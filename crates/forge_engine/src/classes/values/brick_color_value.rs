//! BrickColorValue: holds one BrickColor that scripts can read, change and watch with `Changed`.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::data("Value", "BrickColor", &[194.0]).group("Value").desc("The stored value.")];

pub static CLASS: ClassDesc = ClassDesc::new("BrickColorValue", Some("ValueBase"))
    .describe("Holds a BrickColor.")
    .category("Values")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .groups(&["Value"])
    .components(&["ValueObject"])
    .props(PROPS);