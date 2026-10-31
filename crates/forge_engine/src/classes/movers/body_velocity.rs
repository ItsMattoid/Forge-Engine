//! BodyVelocity: drives a part toward a velocity, using at most `MaxForce` (per axis) to do it. With a large MaxForce
//! the part moves at exactly `Velocity` (gravity does not pull it down on an axis that is driven); with 0 on an axis
//! that axis is left alone. The older mover; `LinearVelocity` does the same with attachments.
use super::mover;
use crate::{
    core::*,
    physics::{bv, fv, StepCtx},
};
static PROPS: &[PropMeta] = &[
    PropMeta::vector3("Velocity", 0.0, 0.0, 0.0).group("BodyVelocity").order(0).drag(0.5).desc("The velocity the part is driven toward, in studs per second."),
    PropMeta::vector3("MaxForce", 4000.0, 4000.0, 4000.0).group("BodyVelocity").order(1).drag(100.0).desc("The most force it may use on each axis (0 leaves the axis alone)."),
    PropMeta::number("P", 1250.0).group("BodyVelocity").order(2).drag(10.0).clamp(0.0, 1.0e9).desc("How hard it works to reach the velocity (kept for scripts; the force limit is what bounds it)."),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "BodyVelocityMover", |dm, m, _part, body, mass, dt| {
        let (target, limit) = (dm.vec3(m, "Velocity"), dm.vec3(m, "MaxForce"));
        let current = fv(body.linear_velocity());
        let next = mover::approach(current, target, limit, mass, dt);
        if next != current {
            body.set_linear_velocity(bv(next));
            body.set_awake(true);
        }
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("BodyVelocityMover").describe("Drives a part toward a velocity.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("BodyVelocity", Some("Instance"))
    .describe("Drives a part toward a velocity.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["BodyVelocity"])
    .components(&["BodyVelocityMover"])
    .props(PROPS);