//! BodyThrust: a constant force in the part's own directions, applied `Location` away from its centre (so it can also
//! turn the part).
use bevy::math::{Quat, Vec3};

use super::mover;
use crate::{
    core::*,
    physics::{bv, StepCtx},
};
static PROPS: &[PropMeta] = &[
    PropMeta::vector3("Force", 0.0, 0.0, 0.0).group("BodyThrust").order(0).drag(10.0).desc("The force, in the part's own directions."),
    PropMeta::vector3("Location", 0.0, 0.0, 0.0).group("BodyThrust").order(1).drag(0.1).desc("Where it pushes, relative to the part's centre."),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "BodyThrustMover", |dm, m, part, body, _mass, _dt| {
        let force = dm.vec3(m, "Force");
        if force == Vec3::ZERO {
            return;
        }
        let (position, rotation) = dm.world_cframe(part).unwrap_or((Vec3::ZERO, Quat::IDENTITY));
        let at = position + rotation * dm.vec3(m, "Location");
        body.apply_force(bv(rotation * force), bv(at), true);
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("BodyThrustMover").describe("Pushes a part with a force in its own directions.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("BodyThrust", Some("Instance"))
    .describe("A constant force on a part, in the part's own directions.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["BodyThrust"])
    .components(&["BodyThrustMover"])
    .props(PROPS);