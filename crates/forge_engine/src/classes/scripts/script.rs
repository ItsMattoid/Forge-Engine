//! Script: runs when it is inside the Workspace or ServerScriptService.
//!
//! `RunContext` is Roblox's choice of side: Legacy (the place decides: a Script runs on the server), Server, or Client
//! (it then counts as a client script: `RunService:IsClient()` is true for it). Where a script *runs* is still decided by
//! where it is in the scene.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::enumeration("RunContext", "RunContext", "Legacy").group("Script").order(3).desc("Which side the script counts as: Legacy, Server or Client.")];

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("RunContext", &["Legacy", "Server", "Client", "Plugin"], &[0, 1, 2, 3])];

pub static CLASS: ClassDesc = ClassDesc::new("Script", Some("BaseScript"))
    .describe("A script that runs in the world.")
    .category("Scripts")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ServerScriptService"))
    .props(PROPS)
    .script_stub("-- `script` is this Script; `script.Parent` is the object it is attached to.\nprint(\"Hello from \" .. script:GetFullName())\n");