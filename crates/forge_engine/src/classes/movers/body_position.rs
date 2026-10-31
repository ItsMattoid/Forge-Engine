//! BodyPosition: pulls a part toward a position like a damped spring - `P` is how stiff, `D` how much it resists
//! moving - using at most `MaxForce` (per axis). With a large MaxForce and the default P and D the part settles on
//! the position within a moment; with 0 on an axis that axis is left alone.
use super::mover;
use crate::{
    core::*,
    physics::{bv, fv, StepCtx},
};
static PROPS: &[PropMeta] = &[
    PropMeta::vector3("Position", 0.0, 0.0, 0.0).group("BodyPosition").order(0).drag(0.5).desc("Where the part is pulled to, in the world."),
    PropMeta::vector3("MaxForce", 4000.0, 4000.0, 4000.0).group("BodyPosition").order(1).drag(100.0).desc("The most force it may use on each axis (0 leaves the axis alone)."),
    PropMeta::number("P", 10_000.0).group("BodyPosition").order(2).drag(100.0).clamp(0.0, 1.0e9).desc("How stiff the pull is."),
    PropMeta::number("D", 1_000.0).group("BodyPosition").order(3).drag(10.0).clamp(0.0, 1.0e9).desc("How much the part resists moving (damping)."),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "BodyPositionMover", |dm, m, part, body, mass, dt| {
        let (stiffness, damping) = (dm.num(m, "P", 10_000.0), dm.num(m, "D", 1_000.0));
        let Some((position, _)) = dm.world_cframe(part) else { return };
        let error = dm.vec3(m, "Position") - position;
        let current = fv(body.linear_velocity());
        // Damping is applied implicitly, so a large D cannot overshoot; the force limit bounds how fast the velocity changes.
        let wanted = (current + error * stiffness * dt) / (1.0 + damping * dt);
        let next = mover::approach(current, wanted, dm.vec3(m, "MaxForce"), mass, dt);
        if next != current {
            body.set_linear_velocity(bv(next));
            body.set_awake(true);
        }
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("BodyPositionMover").describe("Pulls a part toward a position.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("BodyPosition", Some("Instance"))
    .describe("Pulls a part toward a position.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["BodyPosition"])
    .components(&["BodyPositionMover"])
    .props(PROPS);