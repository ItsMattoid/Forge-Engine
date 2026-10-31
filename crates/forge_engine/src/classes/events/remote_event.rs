//! RemoteEvent: an event between the server and the clients (here the same game: see `BaseRemoteEvent`).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("RemoteEvent", Some("BaseRemoteEvent"))
    .describe("An event between the server and a client.")
    .category("Events")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"));