//! Debris: destroys objects after a delay.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("Debris", Some("Instance")).describe("Timed destruction.").service(13).hidden_in_explorer().luau(include_str!("debris.luau"));
