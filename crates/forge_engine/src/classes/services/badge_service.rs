//! BadgeService: Roblox's badges. Nobody has any and none can be awarded.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("BadgeService", Some("Instance"))
    .describe("Roblox's badges (not available in Forge).")
    .service(24)
    .hidden_in_explorer()
    .unsupported(&["GetBadgeInfoAsync"])
    .luau(include_str!("badge_service.luau"));