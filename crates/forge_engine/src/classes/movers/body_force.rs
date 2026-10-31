//! BodyForce: a constant force on the part, in world directions (mass x studs per second squared: a force of
//! `mass * 196.2` upward cancels gravity). Applied at the part's centre of mass every step.
use super::mover;
use crate::{
    core::*,
    physics::{bv, StepCtx},
};
static PROPS: &[PropMeta] = &[PropMeta::vector3("Force", 0.0, 0.0, 0.0).group("BodyForce").order(0).drag(10.0).desc("The force, in world directions.")];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "BodyForceMover", |dm, m, _part, body, _mass, _dt| {
        let force = dm.vec3(m, "Force");
        if force != bevy::math::Vec3::ZERO {
            body.apply_force_to_center(bv(force), true);
        }
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("BodyForceMover").describe("Pushes a part with a constant force.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("BodyForce", Some("Instance"))
    .describe("A constant force on a part.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["BodyForce"])
    .components(&["BodyForceMover"])
    .props(PROPS);