//! InsertService: loading models from Roblox's catalogue. Use `Instance:Clone()` and `ServerStorage`, or prefabs.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("InsertService", Some("Instance"))
    .describe("Loading models from Roblox's catalogue (not available in Forge).")
    .service(37)
    .hidden_in_explorer()
    .unsupported(&["LoadAsset", "LoadAssetVersion", "GetFreeModels", "GetFreeDecals", "GetCollection", "CreateMeshPartAsync"]);