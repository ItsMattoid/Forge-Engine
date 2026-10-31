//! ScriptRunner: marks instances the script host starts (when enabled and located
//! under a service that runs scripts).

use crate::core::*;

pub static COMPONENT: ComponentDesc = ComponentDesc::new("ScriptRunner").describe("Runs its source file as a script.").requires(&["LuaSource"]);
