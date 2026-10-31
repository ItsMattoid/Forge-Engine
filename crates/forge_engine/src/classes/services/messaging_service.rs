//! MessagingService: messages between the servers of a game. A Forge game is one "server", so a message published
//! reaches the handlers subscribed to its topic in this game (and no other).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("MessagingService", Some("Instance"))
    .describe("Messages between servers (here: within this game).")
    .service(21)
    .hidden_in_explorer()
    .luau(include_str!("messaging_service.luau"));