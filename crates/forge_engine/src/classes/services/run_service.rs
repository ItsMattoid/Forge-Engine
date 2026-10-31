//! RunService: per-frame events and facts about how the game is running.
//!
//! Order each frame: input -> Stepped / PreAnimation / PreSimulation -> (physics) -> PostSimulation -> Heartbeat ->
//! RenderStepped / PreRender. (Physics runs at a fixed rate before the frame; see `scripting`.)

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("Stepped").hot().desc("Before physics; passes (time, deltaTime)."),
    SignalDesc::new("Heartbeat").hot().desc("After physics; passes deltaTime."),
    SignalDesc::new("RenderStepped").hot().desc("Just before drawing; passes deltaTime."),
    SignalDesc::new("PreRender").hot().desc("Just before drawing (the newer name of RenderStepped); passes deltaTime."),
    SignalDesc::new("PreAnimation").hot().desc("Before animations and physics; passes deltaTime."),
    SignalDesc::new("PreSimulation").hot().desc("Before physics; passes deltaTime."),
    SignalDesc::new("PostSimulation").hot().desc("After physics; passes deltaTime."),
];

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("RenderPriority", &["First", "Input", "Camera", "Character", "Last"], &[0, 100, 200, 300, 2000])];

pub static CLASS: ClassDesc = ClassDesc::new("RunService", Some("Instance"))
    .describe("Per-frame events.")
    .service(10)
    .hidden_in_explorer()
    .signals(SIGNALS)
    .unsupported(&["Run", "Pause", "Stop", "SetRobloxGuiFocused"])
    .luau(include_str!("run_service.luau"));