//! BaseRemoteEvent: what RemoteEvent and UnreliableRemoteEvent share.
//!
//! There is no network: the "server" and the "client" are the same game. `FireServer` calls the `OnServerEvent`
//! handlers with the local player first (as the server would receive it), `FireClient` / `FireAllClients` call the
//! `OnClientEvent` handlers. Everything is delivered at once, in the order the handlers were connected.

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("OnServerEvent").desc("A client called FireServer; passes (player, ...)."),
    SignalDesc::new("OnClientEvent").desc("The server called FireClient or FireAllClients; passes the arguments."),
];

pub static CLASS: ClassDesc = ClassDesc::new("BaseRemoteEvent", Some("Instance"))
    .describe("An event between the server and a client.")
    .signals(SIGNALS)
    .luau(include_str!("base_remote_event.luau"));