//! TeleportService: moving players between Roblox places. A Forge game is one place (use `game:LoadPlace` to switch
//! scenes); teleporting says it is not supported.

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("TeleportInitFailed").desc("A teleport could not start (never fired).")];

pub static CLASS: ClassDesc = ClassDesc::new("TeleportService", Some("Instance"))
    .describe("Moving players between places (not available in Forge).")
    .service(23)
    .hidden_in_explorer()
    .signals(SIGNALS)
    .unsupported(&["Teleport", "TeleportAsync", "TeleportToPlaceInstance", "TeleportToPrivateServer", "TeleportToSpawnByName", "TeleportPartyAsync", "ReserveServer", "GetPlayerPlaceInstanceAsync", "SetTeleportGui"])
    .luau(include_str!("teleport_service.luau"));