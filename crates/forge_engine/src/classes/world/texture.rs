//! Texture: a picture that repeats over a face of a part, every `StudsPerTileU` x `StudsPerTileV` studs (so it keeps its size on a
//! part of any size), moved by `OffsetStudsU/V`. It is drawn by the same forward decal as `Decal` (see `decal.rs`), which this class
//! builds on.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::number("StudsPerTileU", 2.0).group("Texture").order(0).flags(F_VISUAL).drag(0.1).clamp(0.001, 1.0e6).desc("How wide one repeat of the picture is, in studs."),
    PropMeta::number("StudsPerTileV", 2.0).group("Texture").order(1).flags(F_VISUAL).drag(0.1).clamp(0.001, 1.0e6).desc("How tall one repeat of the picture is, in studs."),
    PropMeta::number("OffsetStudsU", 0.0).group("Texture").order(2).flags(F_VISUAL).drag(0.1).desc("Slides the picture sideways, in studs."),
    PropMeta::number("OffsetStudsV", 0.0).group("Texture").order(3).flags(F_VISUAL).drag(0.1).desc("Slides the picture up or down, in studs."),
];

pub static CLASS: ClassDesc = ClassDesc::new("Texture", Some("Decal"))
    .describe("A picture that repeats over a face of a part.")
    .category("Parts")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a texture belongs to a part." })
    .groups(&["Texture"])
    .props(PROPS);