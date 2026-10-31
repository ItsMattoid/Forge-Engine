//! BodyAngularVelocity: spins a part toward an angular velocity (radians per second, about the world axes), using at
//! most `MaxTorque` (per axis; 0 leaves the axis alone).
use super::mover;
use crate::{
    core::*,
    physics::{bv, fv, StepCtx},
};
static PROPS: &[PropMeta] = &[
    PropMeta::vector3("AngularVelocity", 0.0, 0.0, 0.0).group("BodyAngularVelocity").order(0).drag(0.1).desc("The spin the part is driven toward, in radians per second."),
    PropMeta::vector3("MaxTorque", 4000.0, 4000.0, 4000.0).group("BodyAngularVelocity").order(1).drag(100.0).desc("The most torque it may use on each axis (0 leaves the axis alone)."),
    PropMeta::number("P", 1250.0).group("BodyAngularVelocity").order(2).drag(10.0).clamp(0.0, 1.0e9).desc("How hard it works to reach the spin (kept for scripts)."),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "BodyAngularVelocityMover", |dm, m, part, body, mass, dt| {
        let (target, torque) = (dm.vec3(m, "AngularVelocity"), dm.vec3(m, "MaxTorque"));
        // The inertia of a part is about its mass times the square of its size; this stands in for it.
        let size = dm.size(part).max_element().max(0.1);
        let current = fv(body.angular_velocity());
        let next = mover::approach(current, target, torque, mass * size * size / 4.0, dt);
        if next != current {
            body.set_angular_velocity(bv(next));
            body.set_awake(true);
        }
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("BodyAngularVelocityMover").describe("Spins a part toward an angular velocity.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("BodyAngularVelocity", Some("Instance"))
    .describe("Spins a part toward an angular velocity.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["BodyAngularVelocity"])
    .components(&["BodyAngularVelocityMover"])
    .props(PROPS);