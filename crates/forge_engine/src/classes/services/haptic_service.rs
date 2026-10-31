//! HapticService: controller vibration. Forge reports that no motor exists.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("HapticService", Some("Instance")).describe("Controller vibration (none in Forge).").service(40).hidden_in_explorer().luau(include_str!("haptic_service.luau"));