//! LuaSource: marks instances that have Luau source code (the `SourceFile` property).

use crate::core::*;

pub static COMPONENT: ComponentDesc = ComponentDesc::new("LuaSource").describe("Has Luau source code; can be loaded with require().");
