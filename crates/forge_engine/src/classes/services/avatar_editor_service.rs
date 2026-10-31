//! AvatarEditorService: Roblox avatars.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("AvatarEditorService", Some("Instance"))
    .describe("Roblox avatars (not available in Forge).")
    .service(47)
    .hidden_in_explorer()
    .unsupported(&["GetAvatarRules", "GetInventory", "GetBatchItemDetails", "PromptSaveAvatar", "NoPromptSaveAvatar"]);