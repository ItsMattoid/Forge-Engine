//! UnreliableRemoteEvent: a RemoteEvent that may drop messages on a real network (here it drops none).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("UnreliableRemoteEvent", Some("BaseRemoteEvent"))
    .describe("A RemoteEvent that may lose messages.")
    .category("Events")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"));