//! PVInstance: Roblox's name for "an instance with a position in the world" - the parent of parts and models.
//!
//! The pivot API (`PivotTo`, `GetPivot`, `PivotOffset`) lives in `WorldObject`, which inherits from this class
//! and also serves lights and cameras; being here makes `part:IsA("PVInstance")` true, as in Roblox.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("PVInstance", Some("Instance")).describe("An object that has a position in the world.");
