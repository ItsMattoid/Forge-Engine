//! VRService: virtual reality. Forge has none.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::boolean("VREnabled", false).runtime().hidden().readonly()];

pub static CLASS: ClassDesc = ClassDesc::new("VRService", Some("Instance")).describe("Virtual reality (not available in Forge).").service(32).hidden_in_explorer().props(PROPS);