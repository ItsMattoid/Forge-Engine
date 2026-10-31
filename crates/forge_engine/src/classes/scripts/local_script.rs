//! LocalScript: runs for the local player (it lives in StarterPlayer).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("LocalScript", Some("BaseScript"))
    .describe("A script for the local player (input, camera, UI).")
    .category("Scripts")
    .creatable()
    .parent_rule(ParentRule::Service("StarterPlayer"))
    .script_stub("-- Runs for the local player.\nlocal UserInputService = game:GetService(\"UserInputService\")\n\nprint(\"LocalScript started\")\n");
