//! Motor6D: a weld whose first frame is moved by `Transform` - the joint of an animated character. Nothing here
//! plays animations, but a script can set `Transform` (or `C0`) every frame to pose the parts.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::cframe("Transform").group("Motor6D").order(0).flags(F_PHYS).runtime().desc("A pose the first part is moved by, relative to C0.")];

pub static CLASS: ClassDesc = ClassDesc::new("Motor6D", Some("Weld")).describe("A weld that can be posed with Transform.").category("Constraints").creatable().groups(&["Motor6D"]).props(PROPS);