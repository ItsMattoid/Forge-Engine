//! GroupService: Roblox groups.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("GroupService", Some("Instance"))
    .describe("Roblox groups (not available in Forge).")
    .service(36)
    .hidden_in_explorer()
    .unsupported(&["GetGroupInfoAsync", "GetAlliesAsync", "GetEnemiesAsync", "GetGroupsAsync"]);