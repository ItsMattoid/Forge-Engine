//! IntValue: holds one value that scripts can read, change and watch with `Changed`.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::integer("Value", 0).group("Value").desc("The stored value.")];

pub static CLASS: ClassDesc = ClassDesc::new("IntValue", Some("ValueBase"))
    .describe("Holds a whole number.")
    .category("Values")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .groups(&["Value"])
    .components(&["ValueObject"])
    .props(PROPS);
