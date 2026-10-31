//! PrismaticConstraint: the second part slides along one axis (the X axis of the joint's frame) and cannot turn,
//! like a drawer. With the limits on the slide stays within `LowerLimit` .. `UpperLimit`; with the motor on it is
//! driven at `MotorSpeed` (studs per second), with at most `MotorMaxForce`.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("LimitsEnabled", false).group("Prismatic").order(0).flags(F_PHYS).desc("Whether the slide is limited."),
    PropMeta::number("LowerLimit", 0.0).group("Prismatic").order(1).flags(F_PHYS).drag(0.1).clamp(-1.0e6, 1.0e6).when_true("LimitsEnabled").desc("The smallest slide, in studs."),
    PropMeta::number("UpperLimit", 10.0).group("Prismatic").order(2).flags(F_PHYS).drag(0.1).clamp(-1.0e6, 1.0e6).when_true("LimitsEnabled").desc("The largest slide, in studs."),
    PropMeta::boolean("MotorEnabled", false).group("Prismatic").order(3).flags(F_PHYS).desc("Whether a motor slides the part."),
    PropMeta::number("MotorSpeed", 0.0)
        .group("Prismatic")
        .order(4)
        .flags(F_PHYS)
        .drag(0.1)
        .clamp(-10_000.0, 10_000.0)
        .when_true("MotorEnabled")
        .desc("The speed the motor aims for, in studs per second."),
    PropMeta::number("MotorMaxForce", 10_000.0).group("Prismatic").order(5).flags(F_PHYS).drag(10.0).clamp(0.0, 1.0e9).when_true("MotorEnabled").desc("The most force the motor may use."),
];

pub static CLASS: ClassDesc = ClassDesc::new("PrismaticConstraint", Some("Constraint"))
    .describe("Lets a part slide along one axis, like a drawer.")
    .category("Constraints")
    .creatable()
    .groups(&["Prismatic"])
    .components(&["JointFrames"])
    .props(PROPS);
