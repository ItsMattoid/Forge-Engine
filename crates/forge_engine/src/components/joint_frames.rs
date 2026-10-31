//! JointFrames: where a constraint joins its parts. `Anchor0` / `Rotation0` place the joint's frame in `Part0`
//! (relative to the part's centre and turned with it), `Anchor1` / `Rotation1` place it in `Part1`; the constraint
//! pulls the two frames together. A hinge turns about the frame's X axis, a prismatic constraint slides along it,
//! a ball socket twists about it. `C0` and `C1` are the same frames as CFrames, for scripts.
//!
//! With no `Part1` the second frame is in the world.

use crate::{core::*, dm::DataModel};

fn frame(dm: &DataModel, id: Id, anchor: &str, rotation: &str) -> Value {
    Value::CF(dm.vec3(id, anchor), euler_deg(dm.vec3(id, rotation)))
}

fn set_frame(dm: &mut DataModel, id: Id, anchor: &str, rotation: &str, v: Value) -> Result<(), String> {
    let Value::CF(p, q) = v else { return Err("a constraint frame must be a CFrame".into()) };
    dm.set_prop(id, anchor, Value::V3(p))?;
    dm.set_prop(id, rotation, Value::V3(to_euler_deg(q)))
}

fn get_c0(dm: &DataModel, id: Id) -> Option<Value> {
    Some(frame(dm, id, "Anchor0", "Rotation0"))
}

fn set_c0(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    set_frame(dm, id, "Anchor0", "Rotation0", v)
}

fn get_c1(dm: &DataModel, id: Id) -> Option<Value> {
    Some(frame(dm, id, "Anchor1", "Rotation1"))
}

fn set_c1(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    set_frame(dm, id, "Anchor1", "Rotation1", v)
}

static PROPS: &[PropMeta] = &[
    PropMeta::vector3("Anchor0", 0.0, 0.0, 0.0).group("Constraint").order(10).flags(F_PHYS).drag(0.1).desc("Where the joint is in Part0, from the part's centre, in the part's own axes."),
    PropMeta::vector3("Rotation0", 0.0, 0.0, 0.0)
        .group("Constraint")
        .order(11)
        .flags(F_PHYS)
        .drag(0.5)
        .desc("How the joint's frame is turned in Part0, in degrees. Its X axis is the axis of a hinge, slider or twist."),
    PropMeta::vector3("Anchor1", 0.0, 0.0, 0.0).group("Constraint").order(12).flags(F_PHYS).drag(0.1).desc("Where the joint is in Part1 (or in the world when there is no Part1)."),
    PropMeta::vector3("Rotation1", 0.0, 0.0, 0.0).group("Constraint").order(13).flags(F_PHYS).drag(0.5).desc("How the joint's frame is turned in Part1, in degrees."),
    PropMeta::cframe("C0").hidden().no_save().computed(get_c0, set_c0).desc("Anchor0 and Rotation0 as one CFrame."),
    PropMeta::cframe("C1").hidden().no_save().computed(get_c1, set_c1).desc("Anchor1 and Rotation1 as one CFrame."),
];

pub static COMPONENT: ComponentDesc = ComponentDesc::new("JointFrames").describe("The frames a constraint joins its parts at.").requires(&["Joint"]).props(PROPS);
