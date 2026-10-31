//! ScriptContext: facts about the scripts of the game. `Error` fires when a script raises an error nobody caught
//! (the same errors the output shows); passes (message, stack trace, the script).

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("Error").desc("A script raised an error nobody caught; passes (message, stack trace, script).")];

static PROPS: &[PropMeta] = &[PropMeta::boolean("ScriptsDisabled", false).hidden().readonly()];

pub static CLASS: ClassDesc = ClassDesc::new("ScriptContext", Some("Instance")).describe("Facts about the scripts of the game.").service(34).hidden_in_explorer().props(PROPS).signals(SIGNALS);