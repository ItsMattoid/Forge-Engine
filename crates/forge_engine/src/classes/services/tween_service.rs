//! TweenService: smoothly change properties over time. `TweenService:Create(instance, TweenInfo.new(...), { Property = goal })`
//! makes a `Tween`; `tween:Play()` runs it. The engine is in `tween_service.luau`: it works on any property whose value can
//! be blended - numbers, Vector3, Vector2, Color3, CFrame, UDim2 - with every easing style and direction, delays, repeats
//! and reverses.

use crate::core::*;

pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::numbered("EasingStyle", &["Linear", "Sine", "Back", "Quad", "Quart", "Quint", "Bounce", "Elastic", "Exponential", "Circular", "Cubic"], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
    EnumDesc::numbered("EasingDirection", &["In", "Out", "InOut"], &[0, 1, 2]),
    EnumDesc::numbered("PlaybackState", &["Begin", "Delayed", "Playing", "Paused", "Completed", "Cancelled"], &[0, 1, 2, 3, 4, 5]),
    EnumDesc::numbered("TweenStatus", &["Canceled", "Completed"], &[0, 1]),
];

pub static CLASS: ClassDesc = ClassDesc::new("TweenService", Some("Instance")).describe("Property animation.").service(14).hidden_in_explorer().luau(include_str!("tween_service.luau"));