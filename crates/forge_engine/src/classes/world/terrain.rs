//! Terrain: Roblox's voxel ground. **Forge has no voxel terrain**: the object exists (every Workspace has one, as
//! `workspace.Terrain`) so that scripts which set the water look run, and the settings are kept; the methods that fill,
//! read or erase voxels say they are not supported.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::color3("WaterColor", 0.05, 0.33, 0.36).group("Water").order(0).flags(F_VISUAL).hidden().desc("The colour of water (kept for scripts; there is no water)."),
    PropMeta::number("WaterReflectance", 1.0).group("Water").order(1).flags(F_VISUAL).slider(0.0, 1.0).hidden().desc("How reflective water is (kept for scripts)."),
    PropMeta::number("WaterTransparency", 0.3).group("Water").order(2).flags(F_VISUAL).slider(0.0, 1.0).hidden().desc("How see-through water is (kept for scripts)."),
    PropMeta::number("WaterWaveSize", 0.15).group("Water").order(3).flags(F_VISUAL).slider(0.0, 1.0).hidden().desc("How high the waves are (kept for scripts)."),
    PropMeta::number("WaterWaveSpeed", 10.0).group("Water").order(4).flags(F_VISUAL).slider(0.0, 100.0).hidden().desc("How fast the waves move (kept for scripts)."),
    PropMeta::boolean("Decoration", false).group("Terrain").order(0).hidden().desc("Whether grass decoration is drawn (kept for scripts)."),
    PropMeta::boolean("IsSmooth", true).group("Terrain").order(1).readonly().hidden(),
];

pub static CLASS: ClassDesc = ClassDesc::new("Terrain", Some("Instance"))
    .describe("Roblox's voxel ground (not available in Forge).")
    .scene_object()
    .groups(&["Water", "Terrain"])
    .props(PROPS)
    .unsupported(&[
        "FillBlock", "FillBall", "FillCylinder", "FillWedge", "FillRegion", "ReadVoxels", "WriteVoxels", "Clear", "CopyRegion", "PasteRegion", "ReplaceMaterial", "CountCells", "CellCenterToWorld", "CellCornerToWorld",
        "WorldToCell", "WorldToCellPreferEmpty", "WorldToCellPreferSolid", "GetMaterialColor", "SetMaterialColor", "SmoothRegion", "ConvertToSmooth", "CanSetWaterProperty",
    ]);