//! ProximityPromptService: the signals of every ProximityPrompt in the game, and the switch for all of them.

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("PromptTriggered").desc("A prompt was triggered; passes (prompt, player)."),
    SignalDesc::new("PromptTriggerEnded").desc("A triggered prompt ended; passes (prompt, player)."),
    SignalDesc::new("PromptButtonHoldBegan").desc("A player began holding a prompt's key; passes (prompt, player)."),
    SignalDesc::new("PromptButtonHoldEnded").desc("A player let go of a prompt's key; passes (prompt, player)."),
    SignalDesc::new("PromptShown").desc("A prompt came into range (never fired: prompts draw no interface)."),
    SignalDesc::new("PromptHidden").desc("A prompt went out of range (never fired)."),
];

static PROPS: &[PropMeta] = &[PropMeta::boolean("Enabled", true).group("ProximityPrompts").desc("Turns every prompt in the game off.")];

pub static CLASS: ClassDesc = ClassDesc::new("ProximityPromptService", Some("Instance"))
    .describe("The signals of every ProximityPrompt.")
    .service(29)
    .hidden_in_explorer()
    .groups(&["ProximityPrompts"])
    .props(PROPS)
    .signals(SIGNALS);