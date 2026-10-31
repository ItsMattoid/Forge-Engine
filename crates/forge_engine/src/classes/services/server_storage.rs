//! ServerStorage: objects that are not part of the world until a script moves them into it.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("ServerStorage", Some("Instance")).describe("objects that are not part of the world until a script moves them into it.").service(6).persisted().container();
