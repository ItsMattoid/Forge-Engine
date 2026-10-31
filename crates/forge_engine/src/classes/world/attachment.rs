//! Attachment: a point on a part (or in the world) that other objects attach to - constraints, beams, particle
//! emitters and sounds name attachments rather than parts.
//!
//! Its `CFrame` / `Position` / `Orientation` (and `Axis`, `SecondaryAxis`) are relative to the parent when the parent has a
//! transform (a part), so the attachment follows it; the `World...` members give and set the same thing in the world.

use bevy::math::{Quat, Vec3};

use crate::{core::*, dm::DataModel};

fn local(dm: &DataModel, id: Id) -> Option<(Vec3, Quat)> {
    dm.cframe(id)
}

fn world(dm: &DataModel, id: Id) -> Option<(Vec3, Quat)> {
    dm.world_cframe(id)
}

fn get_axis(dm: &DataModel, id: Id) -> Option<Value> {
    local(dm, id).map(|(_, q)| Value::V3(q * Vec3::X))
}

fn get_secondary_axis(dm: &DataModel, id: Id) -> Option<Value> {
    local(dm, id).map(|(_, q)| Value::V3(q * Vec3::Y))
}

/// Turns the attachment so that its X axis points along `v` (and its Y axis as near to where it was as it can).
fn set_axis(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::V3(axis) = v else { return Err("Axis must be a Vector3".into()) };
    let axis = axis.try_normalize().ok_or("Axis cannot be zero")?;
    let (p, q) = local(dm, id).ok_or("this attachment has no CFrame")?;
    let turn = Quat::from_rotation_arc(q * Vec3::X, axis);
    dm.set_prop(id, "CFrame", Value::CF(p, (turn * q).normalize()))
}

/// Turns the attachment so that its Y axis points along `v`, keeping its X axis as near to where it was as it can.
fn set_secondary_axis(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::V3(axis) = v else { return Err("SecondaryAxis must be a Vector3".into()) };
    let axis = axis.try_normalize().ok_or("SecondaryAxis cannot be zero")?;
    let (p, q) = local(dm, id).ok_or("this attachment has no CFrame")?;
    let turn = Quat::from_rotation_arc(q * Vec3::Y, axis);
    dm.set_prop(id, "CFrame", Value::CF(p, (turn * q).normalize()))
}

fn get_world_position(dm: &DataModel, id: Id) -> Option<Value> {
    world(dm, id).map(|(p, _)| Value::V3(p))
}

fn get_world_cframe(dm: &DataModel, id: Id) -> Option<Value> {
    world(dm, id).map(|(p, q)| Value::CF(p, q))
}

fn get_world_axis(dm: &DataModel, id: Id) -> Option<Value> {
    world(dm, id).map(|(_, q)| Value::V3(q * Vec3::X))
}

fn get_world_secondary_axis(dm: &DataModel, id: Id) -> Option<Value> {
    world(dm, id).map(|(_, q)| Value::V3(q * Vec3::Y))
}

fn get_world_orientation(dm: &DataModel, id: Id) -> Option<Value> {
    world(dm, id).map(|(_, q)| Value::V3(to_euler_deg(q)))
}

/// Stores a world pose as the local one (relative to the parent when it has a transform).
fn set_world(dm: &mut DataModel, id: Id, p: Vec3, q: Quat) -> Result<(), String> {
    let parent = dm.parent(id).filter(|par| dm.has_component(*par, "Transform")).and_then(|par| dm.world_cframe(par));
    let (lp, lq) = match parent {
        Some((pp, pq)) => (pq.inverse() * (p - pp), (pq.inverse() * q).normalize()),
        None => (p, q),
    };
    dm.set_prop(id, "CFrame", Value::CF(lp, lq))
}

fn set_world_position(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::V3(p) = v else { return Err("WorldPosition must be a Vector3".into()) };
    let (_, q) = world(dm, id).ok_or("this attachment has no CFrame")?;
    set_world(dm, id, p, q)
}

fn set_world_cframe(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::CF(p, q) = v else { return Err("WorldCFrame must be a CFrame".into()) };
    set_world(dm, id, p, q)
}

fn set_world_orientation(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::V3(deg) = v else { return Err("WorldOrientation must be a Vector3".into()) };
    let (p, _) = world(dm, id).ok_or("this attachment has no CFrame")?;
    set_world(dm, id, p, euler_deg(deg))
}

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Visible", false).group("Attachment").order(0).desc("Whether the editor draws the attachment (it is never drawn in a running game)."),
    PropMeta::vector3("Axis", 1.0, 0.0, 0.0).group("Attachment").order(1).no_save().hidden().desc("The direction of the attachment's X axis (relative to the parent).").computed(get_axis, set_axis),
    PropMeta::vector3("SecondaryAxis", 0.0, 1.0, 0.0).group("Attachment").order(2).no_save().hidden().desc("The direction of the attachment's Y axis (relative to the parent).").computed(get_secondary_axis, set_secondary_axis),
    PropMeta::vector3("WorldPosition", 0.0, 0.0, 0.0).group("World").order(0).no_save().hidden().desc("Where the attachment is in the world.").computed(get_world_position, set_world_position),
    PropMeta::cframe("WorldCFrame").group("World").order(1).no_save().hidden().desc("The attachment's CFrame in the world.").computed(get_world_cframe, set_world_cframe),
    PropMeta::vector3("WorldOrientation", 0.0, 0.0, 0.0).group("World").order(2).no_save().hidden().desc("The attachment's rotation in the world, in degrees.").computed(get_world_orientation, set_world_orientation),
    PropMeta::vector3("WorldAxis", 1.0, 0.0, 0.0).group("World").order(3).no_save().hidden().desc("The direction of the attachment's X axis in the world.").computed_ro(get_world_axis),
    PropMeta::vector3("WorldSecondaryAxis", 0.0, 1.0, 0.0).group("World").order(4).no_save().hidden().desc("The direction of the attachment's Y axis in the world.").computed_ro(get_world_secondary_axis),
];

pub static CLASS: ClassDesc = ClassDesc::new("Attachment", Some("WorldObject"))
    .describe("A point on a part that constraints, beams and effects attach to.")
    .category("Parts")
    .creatable()
    .relative_transform()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: an attachment belongs to a part." })
    .groups(&["Transform", "Attachment", "World"])
    .components(&["Transform"])
    .props(PROPS);

#[cfg(test)]
mod tests {
    use bevy::math::Vec3;

    use crate::{core::Value, dm::DataModel};

    #[test]
    fn world_members_follow_the_part_and_setting_them_sets_the_local_pose() {
        let mut dm = DataModel::new();
        let ws = dm.workspace;
        let part = dm.create("Part").unwrap();
        dm.set_parent(part, Some(ws)).unwrap();
        dm.set_prop(part, "Position", Value::V3(Vec3::new(10.0, 0.0, 0.0))).unwrap();
        dm.set_prop(part, "Orientation", Value::V3(Vec3::new(0.0, 90.0, 0.0))).unwrap();
        let attachment = dm.create("Attachment").unwrap();
        dm.set_parent(attachment, Some(part)).unwrap();
        dm.set_prop(attachment, "Position", Value::V3(Vec3::new(1.0, 0.0, 0.0))).unwrap();

        // The part is turned a quarter about Y, so the attachment's +X is the world's -Z.
        let at = dm.get_prop(attachment, "WorldPosition").unwrap();
        let Value::V3(at) = at else { panic!("a Vector3") };
        assert!((at - Vec3::new(10.0, 0.0, -1.0)).length() < 1e-4, "{at}");

        dm.set_prop(attachment, "WorldPosition", Value::V3(Vec3::new(10.0, 3.0, 0.0))).unwrap();
        assert!((dm.vec3(attachment, "Position") - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-4, "stored relative to the part: {:?}", dm.vec3(attachment, "Position"));

        // Moving the part moves the attachment.
        dm.set_prop(part, "Position", Value::V3(Vec3::new(0.0, 0.0, 0.0))).unwrap();
        let Some(Value::V3(at)) = dm.get_prop(attachment, "WorldPosition") else { panic!("a Vector3") };
        assert!((at - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-4);

        // Axis is the direction of the X axis, and can be set.
        dm.set_prop(attachment, "Axis", Value::V3(Vec3::Y)).unwrap();
        let Some(Value::V3(axis)) = dm.get_prop(attachment, "Axis") else { panic!("a Vector3") };
        assert!((axis - Vec3::Y).length() < 1e-4);
    }
}