//! BindableFunction: a function scripts make themselves. Assign `OnInvoke`; `fn:Invoke(...)` calls it and returns
//! what it returns (the caller waits if the function yields).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("BindableFunction", Some("Instance"))
    .describe("A function scripts can set and call.")
    .category("Events")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .callbacks(&["OnInvoke"])
    .luau(include_str!("bindable_function.luau"));