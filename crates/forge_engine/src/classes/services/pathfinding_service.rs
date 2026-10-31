//! PathfindingService: routes through the world. Forge has no navigation mesh yet.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("PathfindingService", Some("Instance"))
    .describe("Routes through the world (Forge has no navigation mesh).")
    .service(28)
    .hidden_in_explorer()
    .unsupported(&["CreatePath", "FindPathAsync"]);