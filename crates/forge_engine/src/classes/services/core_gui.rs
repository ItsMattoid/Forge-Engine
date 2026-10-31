//! CoreGui: Roblox's own interface (the menu, the player list ...). Forge draws none; scripts may still find the
//! service and put things in it.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("CoreGui", Some("Instance")).describe("Roblox's own interface (none in Forge).").service(45).hidden_in_explorer().container();