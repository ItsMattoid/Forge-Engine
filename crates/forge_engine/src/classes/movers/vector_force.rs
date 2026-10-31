//! VectorForce: a force on a part, in the directions of its attachment (`RelativeTo` Attachment0), of the world, or of
//! `Attachment1`. Applied at the attachment unless `ApplyAtCenterOfMass` (so an off-centre force also turns the part).
//! The force is in mass x studs per second squared, applied every step.

use bevy::math::Vec3;

use super::mover;
use crate::{
    core::*,
    physics::{bv, StepCtx},
};

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("ActuatorRelativeTo", &["Attachment0", "Attachment1", "World"], &[0, 1, 2])];

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("VectorForce").order(0).desc("Whether the force is applied."),
    PropMeta::reference("Attachment0", "Attachment").group("VectorForce").order(1).desc("Where the force is applied (and whose part it pushes)."),
    PropMeta::reference("Attachment1", "Attachment").group("VectorForce").order(2).desc("The attachment the force is relative to when RelativeTo is Attachment1."),
    PropMeta::vector3("Force", 1000.0, 0.0, 0.0).group("VectorForce").order(3).drag(10.0).desc("The force, in the directions RelativeTo says."),
    PropMeta::enumeration("RelativeTo", "ActuatorRelativeTo", "Attachment0").group("VectorForce").order(4).desc("Which directions the force is given in."),
    PropMeta::boolean("ApplyAtCenterOfMass", false).group("VectorForce").order(5).desc("Whether the force pushes at the centre (it never turns the part) instead of at the attachment."),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "VectorForceMover", |dm, m, part, body, _mass, _dt| {
        if !dm.flag(m, "Enabled") {
            return;
        }
        let force = dm.vec3(m, "Force");
        if force == Vec3::ZERO {
            return;
        }
        let own = mover::frame_of(dm, m, part, "Attachment0");
        let basis = match dm.string(m, "RelativeTo").as_str() {
            "World" => bevy::math::Quat::IDENTITY,
            "Attachment1" => mover::frame_of(dm, m, part, "Attachment1").1,
            _ => own.1,
        };
        let world = basis * force;
        if dm.flag(m, "ApplyAtCenterOfMass") {
            body.apply_force_to_center(bv(world), true);
        } else {
            body.apply_force(bv(world), bv(own.0), true);
        }
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("VectorForceMover").describe("Pushes a part with a force.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("VectorForce", Some("Instance"))
    .describe("A force on a part, relative to an attachment or the world.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["VectorForce"])
    .components(&["VectorForceMover"])
    .props(PROPS);