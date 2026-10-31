//! BallSocketConstraint: the two parts are fixed at a point and turn freely about it, like a shoulder. The cone
//! limit keeps the second part's axis within `UpperAngle` of the first's; the twist limit bounds the turn about it.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("LimitsEnabled", false).group("Ball socket").order(0).flags(F_PHYS).desc("Whether the swing (the angle between the two axes) is limited."),
    PropMeta::number("UpperAngle", 45.0).group("Ball socket").order(1).flags(F_PHYS).slider(0.0, 180.0).when_true("LimitsEnabled").desc("The most the axes may swing apart, in degrees."),
    PropMeta::boolean("TwistLimitsEnabled", false).group("Ball socket").order(2).flags(F_PHYS).desc("Whether the turn about the axis is limited."),
    PropMeta::number("TwistLowerAngle", -45.0).group("Ball socket").order(3).flags(F_PHYS).slider(-178.0, 178.0).when_true("TwistLimitsEnabled").desc("The smallest twist, in degrees."),
    PropMeta::number("TwistUpperAngle", 45.0).group("Ball socket").order(4).flags(F_PHYS).slider(-178.0, 178.0).when_true("TwistLimitsEnabled").desc("The largest twist, in degrees."),
];

pub static CLASS: ClassDesc = ClassDesc::new("BallSocketConstraint", Some("Constraint"))
    .describe("Fixes two parts at a point and lets them turn freely about it, like a shoulder.")
    .category("Constraints")
    .creatable()
    .groups(&["Ball socket"])
    .components(&["JointFrames"])
    .props(PROPS);
