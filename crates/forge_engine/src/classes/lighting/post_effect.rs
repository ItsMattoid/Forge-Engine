//! PostEffect: what every screen effect in Lighting has - a switch. (Forge keeps the effects' settings and gives
//! scripts the whole API; the renderer does not apply them yet.)

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::boolean("Enabled", true).group("Effect").order(0).flags(F_VISUAL).desc("Whether the effect is on.")];

pub static CLASS: ClassDesc = ClassDesc::new("PostEffect", Some("Instance")).describe("A screen effect.").groups(&["Effect"]).props(PROPS);