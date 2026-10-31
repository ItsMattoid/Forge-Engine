//! PolicyService: which features a player may use. Nothing is restricted.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("PolicyService", Some("Instance")).describe("Which features a player may use (all of them).").service(41).hidden_in_explorer().luau(include_str!("policy_service.luau"));