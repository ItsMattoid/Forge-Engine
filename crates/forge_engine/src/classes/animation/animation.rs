//! Animation: a reference to an animation to play (`AnimationId`). Forge has no animation system: an Animation can be
//! loaded into an Animator, whose tracks keep Roblox''s state and signals (see `Animator`) but pose nothing.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::asset("AnimationId", "anim").group("Animation").order(0).desc("The animation (a project file or an rbxassetid:// string; nothing plays it).")];

pub static CLASS: ClassDesc = ClassDesc::new("Animation", Some("Instance")).describe("A reference to an animation.").category("Characters").creatable().groups(&["Animation"]).props(PROPS);