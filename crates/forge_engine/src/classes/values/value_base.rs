//! ValueBase: what the value objects (`IntValue`, `StringValue`, `Color3Value` ...) have in common. Each holds one
//! `Value` that scripts read, change and watch with `Changed` (which passes the new value).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("ValueBase", Some("Instance")).describe("An object that holds one value.").components(&["ValueObject"]);