//! SpringConstraint: pulls the two frames toward `FreeLength` studs apart, like a coil spring. `Frequency` is how
//! stiff it is (how many times a second it would bounce) and `DampingRatio` how quickly the bouncing dies away (0 is
//! never, 1 is as fast as it can without overshooting). With the limits on it is never shorter than `MinLength` nor
//! longer than `MaxLength`.
//!
//! Roblox describes a spring by `Stiffness` (force per stud) and `Damping` (force per stud per second) instead. When
//! `Stiffness` is above 0 the spring uses them: the frequency and damping ratio are worked out from them and the mass the
//! spring moves (the lighter of the two parts, or the one part when the other end is the world), as a spring of that
//! stiffness would behave. At 0 (the default) `Frequency` and `DampingRatio` rule.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::number("CurrentLength", 0.0).group("Spring").order(8).no_save().hidden().computed_ro(super::rod_constraint::get_current_length).desc("How far apart the two frames are right now."),
    PropMeta::number("FreeLength", 5.0).group("Spring").order(0).flags(F_PHYS).drag(0.1).clamp(0.01, 1.0e6).desc("The length the spring rests at, in studs."),
    PropMeta::number("Frequency", 3.0).group("Spring").order(1).flags(F_PHYS).drag(0.05).clamp(0.0, 1000.0).desc("How stiff the spring is: the times a second it would bounce. 0 is no spring at all."),
    PropMeta::number("DampingRatio", 0.5)
        .group("Spring")
        .order(2)
        .flags(F_PHYS)
        .drag(0.01)
        .clamp(0.0, 100.0)
        .desc("How quickly the bouncing dies away: 0 never, 1 as fast as it can without overshooting."),
    PropMeta::number("Stiffness", 0.0).group("Spring").order(6).flags(F_PHYS).drag(1.0).clamp(0.0, 1.0e9).desc("Roblox's spring strength: force per stud of stretch. When above 0 it is used instead of Frequency."),
    PropMeta::number("Damping", 0.0).group("Spring").order(7).flags(F_PHYS).drag(0.1).clamp(0.0, 1.0e9).desc("Roblox's damping: force per stud per second. Used together with Stiffness instead of DampingRatio."),
    PropMeta::boolean("LimitsEnabled", false).group("Spring").order(3).flags(F_PHYS).desc("Whether the length is limited."),
    PropMeta::number("MinLength", 0.0).group("Spring").order(4).flags(F_PHYS).drag(0.1).clamp(0.0, 1.0e6).when_true("LimitsEnabled").desc("The shortest the spring can be."),
    PropMeta::number("MaxLength", 10.0).group("Spring").order(5).flags(F_PHYS).drag(0.1).clamp(0.0, 1.0e6).when_true("LimitsEnabled").desc("The longest the spring can be."),
];

pub static CLASS: ClassDesc = ClassDesc::new("SpringConstraint", Some("Constraint"))
    .describe("Joins two parts with a spring.")
    .category("Constraints")
    .creatable()
    .groups(&["Spring"])
    .components(&["JointFrames"])
    .props(PROPS)
    .methods(super::rod_constraint::METHODS);
