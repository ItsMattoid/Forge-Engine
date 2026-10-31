//! LuaSourceContainer: anything that holds Luau source code (a file in the project).

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::script_path("SourceFile").group("Script").order(0).flags(F_SCRIPT).desc("The .luau file this script runs.")];

pub static CLASS: ClassDesc = ClassDesc::new("LuaSourceContainer", Some("Instance"))
    .describe("Holds Luau source code.")
    .groups(&["Script"])
    .components(&["LuaSource"])
    .props(PROPS);
