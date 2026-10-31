//! BoolValue: holds one value that scripts can read, change and watch with `Changed`.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::boolean("Value", false).group("Value").desc("The stored value.")];

pub static CLASS: ClassDesc = ClassDesc::new("BoolValue", Some("ValueBase"))
    .describe("Holds a true/false value.")
    .category("Values")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .groups(&["Value"])
    .components(&["ValueObject"])
    .props(PROPS);
