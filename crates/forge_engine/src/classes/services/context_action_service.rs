//! ContextActionService: names for what the player's input does. `BindAction(name, handler, createButton, ...)`
//! calls the handler with `(name, inputState, inputObject)` when one of the listed keys or buttons is used; handlers
//! with a higher priority (and, at the same priority, bound later) are asked first, and the first that does not return
//! `Enum.ContextActionResult.Pass` takes the input. The on-screen buttons and titles of Roblox are not drawn.

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("BoundActionAdded").desc("An action was bound; passes (name, createdButton, info)."),
    SignalDesc::new("BoundActionChanged").desc("A bound action changed."),
    SignalDesc::new("BoundActionRemoved").desc("An action was unbound; passes (name, info)."),
    SignalDesc::new("LocalToolEquipped").desc("The local player equipped a tool; passes the tool."),
    SignalDesc::new("LocalToolUnequipped").desc("The local player put a tool away."),
];

pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::numbered("ContextActionResult", &["Sink", "Pass"], &[0, 1]),
    EnumDesc::numbered("ContextActionPriority", &["Low", "Medium", "High"], &[1000, 2000, 3000]).aliases(&[("Default", "Medium")]),
];

pub static CLASS: ClassDesc = ClassDesc::new("ContextActionService", Some("Instance"))
    .describe("Names for what the player's input does.")
    .service(19)
    .hidden_in_explorer()
    .signals(SIGNALS)
    .luau(include_str!("context_action_service.luau"));