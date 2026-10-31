//! PlayerGui: where a player's interface lives. (Forge has no GUI classes yet, so it only holds what scripts put
//! in it.)

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("PlayerGui", Some("Instance")).describe("The container of a player's interface.");