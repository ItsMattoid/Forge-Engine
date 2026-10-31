//! FaceInstance: what a decal or texture has in common - the face of the part it is on.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::enumeration("Face", "NormalId", "Front").group("Decal").order(0).flags(F_VISUAL).desc("Which side of the part it is on.")];

pub static CLASS: ClassDesc = ClassDesc::new("FaceInstance", Some("Instance")).describe("Something on one face of a part.").relative_transform().groups(&["Decal"]).props(PROPS);