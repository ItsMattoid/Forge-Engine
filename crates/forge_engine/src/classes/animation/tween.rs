//! Tween: one animation made by `TweenService:Create`. `Play`, `Pause` and `Cancel` control it (the engine is in
//! `services/tween_service.luau`); `PlaybackState` says where it is and `Completed` fires with the state it ended in.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::reference("Instance", "Instance").group("Tween").order(0).runtime().desc("The object being tweened."),
    PropMeta::enumeration("PlaybackState", "PlaybackState", "Begin").group("Tween").order(1).runtime().desc("Where the tween is: Begin, Delayed, Playing, Paused, Completed or Cancelled."),
];

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("Completed").desc("The tween ended; passes Enum.PlaybackState.Completed, or Cancelled if it was cancelled.")];

pub static CLASS: ClassDesc = ClassDesc::new("Tween", Some("Instance"))
    .describe("One animation of properties over time.")
    .groups(&["Tween"])
    .props(PROPS)
    .signals(SIGNALS)
    .luau(include_str!("tween.luau"));