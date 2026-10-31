//! ModuleScript: reusable code; run once by `require(moduleScript)`.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("ModuleScript", Some("LuaSourceContainer"))
    .describe("Reusable code loaded with require().")
    .category("Scripts")
    .creatable()
    .parent_rule(ParentRule::Service("ReplicatedStorage"))
    .script_stub("local Module = {}\n\nfunction Module.hello(name)\n\tprint(\"Hello, \" .. name)\nend\n\nreturn Module\n");
