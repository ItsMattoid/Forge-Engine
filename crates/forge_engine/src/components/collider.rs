//! Collider: how a part meets other parts - whether it collides, can be touched, can be found by queries, and
//! which collision group it is in - and the collision volume the editor selects and draws.

use crate::{
    core::*,
    dm::DataModel,
    physics::{groups, ColliderInfo, ColliderShape},
};

/// A collision group name has to be one that could be registered (the group itself need not exist yet: a scene
/// file may name the groups before `PhysicsService` has loaded them, and a part in a group that does not exist
/// behaves as if it were in `Default`, with a warning in the log).
fn check_group(_dm: &DataModel, _id: Id, v: &Value) -> Result<(), String> {
    match v {
        Value::Str(name) => match groups::name_problem(name) {
            Some(problem) => Err(problem),
            None => Ok(()),
        },
        _ => Ok(()),
    }
}

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("CanCollide", true)
        .group("Collision")
        .order(0)
        .flags(F_PHYS)
        .desc("Whether other parts collide with this one. A part that does not collide can still be touched and found by queries."),
    PropMeta::boolean("CanTouch", true).group("Collision").order(1).flags(F_PHYS).desc("Whether Touched / TouchEnded fire for this part (both parts of a pair must allow it)."),
    PropMeta::boolean("CanQuery", true).group("Collision").order(2).flags(F_PHYS).desc("Whether ray casts, shape casts and overlap tests can find this part."),
    PropMeta::string("CollisionGroup", "Default")
        .group("Collision")
        .order(3)
        .flags(F_PHYS)
        .validate(check_group)
        .desc("The collision group the part is in: it collides with the groups its group is set to collide with (see PhysicsService)."),
];

fn collider(dm: &DataModel, id: Id) -> Option<ColliderInfo> {
    let (center, rotation) = dm.world_cframe(id)?;
    Some(ColliderInfo {
        center,
        rotation,
        size: dm.size(id),
        shape: match dm.str_ref(id, "Shape") {
            "Ball" => ColliderShape::Ball,
            "Wedge" => ColliderShape::Wedge,
            "CornerWedge" => ColliderShape::CornerWedge,
            _ => ColliderShape::Box,
        },
        can_collide: dm.flag(id, "CanCollide"),
        can_touch: dm.flag(id, "CanTouch"),
        anchored: dm.flag(id, "Anchored"),
    })
}

fn gizmo(dm: &DataModel, id: Id, out: &mut Vec<GizmoShape>) {
    if let Some(i) = dm.collider(id) {
        out.push(GizmoShape::Box { center: i.center, rotation: i.rotation, size: i.size });
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("Collider").describe("A collision volume.").requires(&["Transform"]).props(PROPS).collider(collider).gizmo(gizmo);
