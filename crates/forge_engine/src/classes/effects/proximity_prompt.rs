//! ProximityPrompt: "press E to ...". Pressing the prompt's `KeyboardKeyCode` while the player's character is within
//! `MaxActivationDistance` studs of the part (or attachment) it is in fires `Triggered(player)` (after `HoldDuration`
//! seconds of holding the key, if that is more than 0) - and `ProximityPromptService.PromptTriggered`. Forge draws no
//! prompt, so nothing tells the player the key; `PromptShown` and `PromptHidden` never fire.

use crate::core::*;

pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::new("ProximityPromptStyle", &["Default", "Custom"]),
    EnumDesc::new("ProximityPromptExclusivity", &["OnePerButton", "OneGlobally", "AlwaysShow"]),
];

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Prompt").order(0).desc("Whether the prompt works."),
    PropMeta::string("ActionText", "").group("Prompt").order(1).desc("What pressing does (shown to the player)."),
    PropMeta::string("ObjectText", "").group("Prompt").order(2).desc("What it is done to (shown to the player)."),
    PropMeta::number("HoldDuration", 0.0).group("Prompt").order(3).drag(0.1).clamp(0.0, 60.0).desc("Seconds the key must be held."),
    PropMeta::number("MaxActivationDistance", 10.0).group("Prompt").order(4).drag(0.5).clamp(0.0, 1.0e5).desc("How near the player's character must be, in studs."),
    PropMeta::enumeration("KeyboardKeyCode", "KeyCode", "E").group("Prompt").order(5).desc("The key that triggers the prompt."),
    PropMeta::boolean("RequiresLineOfSight", true).group("Prompt").order(6).desc("Whether nothing may be between the player and the prompt (not checked)."),
    PropMeta::enumeration("Style", "ProximityPromptStyle", "Default").group("Prompt").order(7).hidden(),
    PropMeta::enumeration("Exclusivity", "ProximityPromptExclusivity", "OnePerButton").group("Prompt").order(8).hidden(),
    PropMeta::vector2("UIOffset", 0.0, 0.0).group("Prompt").order(9).hidden(),
    PropMeta::boolean("ClickablePrompt", true).group("Prompt").order(10).hidden(),
];

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("Triggered").desc("The prompt was triggered; passes the player."),
    SignalDesc::new("TriggerEnded").desc("The key was let go after the prompt triggered; passes the player."),
    SignalDesc::new("PromptButtonHoldBegan").desc("The player began holding the key; passes the player."),
    SignalDesc::new("PromptButtonHoldEnded").desc("The player let go of the key; passes the player."),
    SignalDesc::new("PromptShown").desc("The prompt came into range (never fired: nothing is drawn)."),
    SignalDesc::new("PromptHidden").desc("The prompt went out of range (never fired)."),
];

pub static CLASS: ClassDesc = ClassDesc::new("ProximityPrompt", Some("Instance"))
    .describe("A key the player presses near an object.")
    .category("Effects")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "WorldObject", hint: "Select a part or an attachment first." })
    .luau(include_str!("proximity_prompt.luau"))
    .groups(&["Prompt"])
    .props(PROPS)
    .signals(SIGNALS);