//! ValueObject: marks "value holder" instances (BoolValue, NumberValue, ...). Their
//! `Changed` signal passes the new value instead of the property name.

use crate::core::*;

pub static COMPONENT: ComponentDesc = ComponentDesc::new("ValueObject").describe("Holds a single value in its Value property.");
