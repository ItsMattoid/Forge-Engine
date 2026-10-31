//! RemoteFunction: a function the server and a client can call on each other (here the same game).
//! `InvokeServer(...)` calls `OnServerInvoke(player, ...)`, `InvokeClient(player, ...)` calls `OnClientInvoke(...)`,
//! and each returns what the callback returns.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("RemoteFunction", Some("Instance"))
    .describe("A function between the server and a client.")
    .category("Events")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .callbacks(&["OnServerInvoke", "OnClientInvoke"])
    .luau(include_str!("remote_function.luau"));