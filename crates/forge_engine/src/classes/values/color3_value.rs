//! Color3Value: holds one colour that scripts can read, change and watch with `Changed`.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::color3("Value", 0.0, 0.0, 0.0).group("Value").desc("The stored value.")];

pub static CLASS: ClassDesc = ClassDesc::new("Color3Value", Some("ValueBase"))
    .describe("Holds a colour.")
    .category("Values")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .groups(&["Value"])
    .components(&["ValueObject"])
    .props(PROPS);