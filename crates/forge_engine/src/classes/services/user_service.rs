//! UserService: information about users. Only the local player is known.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("UserService", Some("Instance")).describe("Information about users (only the local player).").service(42).hidden_in_explorer().luau(include_str!("user_service.luau"));