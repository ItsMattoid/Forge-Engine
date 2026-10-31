//! StringValue: holds one value that scripts can read, change and watch with `Changed`.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::string("Value", "").group("Value").desc("The stored value.")];

pub static CLASS: ClassDesc = ClassDesc::new("StringValue", Some("ValueBase"))
    .describe("Holds text.")
    .category("Values")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .groups(&["Value"])
    .components(&["ValueObject"])
    .props(PROPS);
