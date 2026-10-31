//! Transform: a world-space `CFrame` plus the convenient `Position` / `Orientation`
//! views of it. Used by everything that exists at a place in the world.

use crate::core::*;
use crate::dm::DataModel;

fn get_position(dm: &DataModel, id: Id) -> Option<Value> {
    dm.cframe(id).map(|(p, _)| Value::V3(p))
}

fn set_position(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::V3(p) = v else { return Err("Position must be a Vector3".into()) };
    let (_, q) = dm.cframe(id).ok_or("this object has no CFrame")?;
    dm.set_prop(id, "CFrame", Value::CF(p, q))
}

fn get_orientation(dm: &DataModel, id: Id) -> Option<Value> {
    dm.cframe(id).map(|(_, q)| Value::V3(to_euler_deg(q)))
}

fn set_orientation(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::V3(deg) = v else { return Err("Orientation must be a Vector3".into()) };
    let (p, _) = dm.cframe(id).ok_or("this object has no CFrame")?;
    dm.set_prop(id, "CFrame", Value::CF(p, euler_deg(deg)))
}

static PROPS: &[PropMeta] = &[
    PropMeta::cframe("CFrame").flags(F_XFORM | F_PHYS).hidden().no_save().desc("Position and rotation together."),
    PropMeta::vector3("Position", 0.0, 0.0, 0.0)
        .group("Transform")
        .order(0)
        .drag(0.1)
        .desc("Where the object is.")
        .computed(get_position, set_position),
    PropMeta::vector3("Orientation", 0.0, 0.0, 0.0)
        .label("Rotation")
        .group("Transform")
        .order(1)
        .drag(0.5)
        .aliases(&["Rotation"])
        .desc("Rotation in degrees (X = pitch, Y = yaw, Z = roll).")
        .computed(get_orientation, set_orientation),
];

/// When an object moves, objects attached to it (relative transforms) move too.
fn on_changed(dm: &mut DataModel, id: Id, prop: &PropMeta) {
    if prop.name == "CFrame" {
        dm.mark_dependents(id);
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("Transform").describe("A position and rotation in the world.").props(PROPS).on_changed(on_changed);
