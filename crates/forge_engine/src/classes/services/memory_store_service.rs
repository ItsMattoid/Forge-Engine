//! MemoryStoreService: Roblox's shared in-memory stores (they live on its servers).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("MemoryStoreService", Some("Instance"))
    .describe("Roblox's shared memory stores (not available in Forge).")
    .service(27)
    .hidden_in_explorer()
    .unsupported(&["GetSortedMap", "GetQueue", "GetHashMap"]);