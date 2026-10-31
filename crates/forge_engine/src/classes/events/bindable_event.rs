//! BindableEvent: an event scripts make themselves. `event:Fire(...)` calls everything connected to `event.Event`.
//!
//! Handlers run at once, in the order they were connected, each in its own thread (so they may yield). Arguments are
//! handed over as they are (Roblox copies tables; here both sides see the same table).

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("Event").desc("Fires when Fire is called; passes the arguments of Fire.")];

pub static CLASS: ClassDesc = ClassDesc::new("BindableEvent", Some("Instance"))
    .describe("An event scripts can fire and listen to.")
    .category("Events")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("ReplicatedStorage"))
    .signals(SIGNALS)
    .luau(include_str!("bindable_event.luau"));