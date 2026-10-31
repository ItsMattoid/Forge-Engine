//! ObjectValue: holds one value that scripts can read, change and watch with `Changed`.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::reference("Value", "Instance").group("Value").desc("The stored value.")];

pub static CLASS: ClassDesc = ClassDesc::new("ObjectValue", Some("ValueBase"))
    .describe("Holds a reference to another object.")
    .category("Values")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .groups(&["Value"])
    .components(&["ValueObject"])
    .props(PROPS);
