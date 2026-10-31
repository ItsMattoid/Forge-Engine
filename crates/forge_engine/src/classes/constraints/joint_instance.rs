//! JointInstance: the base of the joints that hold two parts at the places their frames (C0, C1) say - Weld,
//! ManualWeld, Motor6D. (Roblox's Weld is not a constraint, but here it joins parts by the same machinery.)

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("JointInstance", Some("Constraint")).describe("The base of the joints with frames: Weld, ManualWeld, Motor6D.").category("Constraints");
