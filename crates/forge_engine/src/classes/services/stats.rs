//! Stats: counters of the running game. Memory comes from the Luau VM; the rest of Roblox's performance counters are
//! not kept (the debug overlays of the editor and player show Forge's own).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("Stats", Some("Instance")).describe("Counters of the running game.").service(33).hidden_in_explorer().luau(include_str!("stats.luau"));