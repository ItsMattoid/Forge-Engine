//! DataStoreService: named stores of values that outlive a script's run. Here they live in memory for the length
//! of the game (a fresh run starts empty): there is no cloud to keep them in. The stores behave as Roblox's do -
//! values are copied through JSON (so only booleans, numbers, strings and tables of them can be stored), `UpdateAsync`
//! may cancel by returning nil, ordered stores keep numbers and can be listed in order.

use crate::core::*;

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("DataStoreRequestType", &["GetAsync", "GetSortedAsync", "SetIncrementAsync", "SetIncrementSortedAsync", "UpdateAsync", "GetVersionAsync", "ListAsync"], &[0, 1, 2, 3, 4, 5, 6])];

pub static CLASS: ClassDesc = ClassDesc::new("DataStoreService", Some("Instance"))
    .describe("Named stores of values (kept in memory for the length of the game).")
    .service(20)
    .hidden_in_explorer()
    .unsupported(&["ListDataStoresAsync"])
    .luau(include_str!("data_store_service.luau"));