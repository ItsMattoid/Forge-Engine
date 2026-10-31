//! ServerScriptService: where Scripts live and run.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("ServerScriptService", Some("Instance")).describe("where Scripts live and run.").service(5).persisted().runs_scripts().container();
