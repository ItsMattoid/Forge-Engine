//! ReplicatedStorage: a place for shared objects and values.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("ReplicatedStorage", Some("Instance")).describe("a place for shared objects and values.").service(4).persisted().container();
