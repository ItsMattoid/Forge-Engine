//! AssetService: places and assets on Roblox.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("AssetService", Some("Instance"))
    .describe("Places and assets on Roblox (not available in Forge).")
    .service(38)
    .hidden_in_explorer()
    .unsupported(&["CreatePlaceAsync", "CreatePlaceInPlayerInventoryAsync", "GetGamePlacesAsync", "SavePlaceAsync", "CreateEditableImage", "CreateEditableMesh"]);