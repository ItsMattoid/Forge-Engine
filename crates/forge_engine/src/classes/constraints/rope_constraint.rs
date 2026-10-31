//! RopeConstraint: the two frames can come together but never be farther than `Length` studs apart.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::number("Length", 10.0).group("Rope").order(0).flags(F_PHYS).drag(0.1).clamp(0.01, 1.0e6).desc("The longest the rope can be, in studs."),
    PropMeta::number("CurrentDistance", 0.0).group("Rope").order(1).no_save().hidden().computed_ro(super::rod_constraint::get_current_length).desc("How far apart the two frames are right now."),
];

pub static CLASS: ClassDesc = ClassDesc::new("RopeConstraint", Some("Constraint"))
    .describe("Joins two parts with a rope: slack up to its length, then taut.")
    .category("Constraints")
    .creatable()
    .groups(&["Rope"])
    .components(&["JointFrames"])
    .props(PROPS)
    .methods(super::rod_constraint::METHODS);
