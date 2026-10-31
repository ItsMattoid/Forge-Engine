//! LogService: what the game prints. `MessageOut` fires for every line that `print`, `warn` and uncaught script errors
//! put in the output; passes (message, Enum.MessageType).

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("MessageOut").desc("A line was written to the output; passes (message, message type).")];

pub static CLASS: ClassDesc = ClassDesc::new("LogService", Some("Instance"))
    .describe("What the game prints.")
    .service(35)
    .hidden_in_explorer()
    .signals(SIGNALS)
    .luau(include_str!("log_service.luau"));