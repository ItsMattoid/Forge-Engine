//! SurfaceLight: light that comes out of one face of a part (`Face`) in a cone (`Angle`). It is a spot light that points
//! out of the face, so it is drawn by the same emitter.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("SurfaceLight", Some("SpotLight"))
    .describe("Light that comes out of one face of a part.")
    .category("Lights")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a surface light shines out of one of its faces." });