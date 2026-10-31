//! NumberValue: holds one value that scripts can read, change and watch with `Changed`.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::number("Value", 0.0).group("Value").desc("The stored value.")];

pub static CLASS: ClassDesc = ClassDesc::new("NumberValue", Some("ValueBase"))
    .describe("Holds a number.")
    .category("Values")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .groups(&["Value"])
    .components(&["ValueObject"])
    .props(PROPS);
