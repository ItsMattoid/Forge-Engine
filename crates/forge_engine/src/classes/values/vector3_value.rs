//! Vector3Value: holds one vector that scripts can read, change and watch with `Changed`.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::vector3("Value", 0.0, 0.0, 0.0).group("Value").drag(0.1).desc("The stored value.")];

pub static CLASS: ClassDesc = ClassDesc::new("Vector3Value", Some("ValueBase"))
    .describe("Holds a vector.")
    .category("Values")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .groups(&["Value"])
    .components(&["ValueObject"])
    .props(PROPS);