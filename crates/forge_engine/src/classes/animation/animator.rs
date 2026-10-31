//! Animator: plays animations on a model. **Forge has no animation playback**: `LoadAnimation` returns an
//! AnimationTrack that behaves as Roblox''s does for a script - `Play`, `Stop`, `AdjustSpeed`, `IsPlaying`,
//! `TimePosition`, `Stopped`, `Ended`, `DidLoop` - but moves no part. An animation has no known length, so a track that is
//! not `Looped` ends as soon as it starts.

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("AnimationPlayed").desc("A track started playing; passes the track.")];

pub static CLASS: ClassDesc = ClassDesc::new("Animator", Some("Instance"))
    .describe("Plays animations (without posing anything).")
    .category("Characters")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "Model", hint: "Select a Model first: an Animator animates the model it is in." })
    .signals(SIGNALS)
    .luau(include_str!("animator.luau"));