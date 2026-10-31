//! Torque: a turning force on a part, in the directions of its attachment, of the world, or of `Attachment1`.

use bevy::math::Vec3;

use super::mover;
use crate::{
    core::*,
    physics::{bv, StepCtx},
};

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Torque").order(0).desc("Whether the torque is applied."),
    PropMeta::reference("Attachment0", "Attachment").group("Torque").order(1).desc("Whose part is turned, and whose directions Attachment0 means."),
    PropMeta::reference("Attachment1", "Attachment").group("Torque").order(2).desc("The attachment the torque is relative to when RelativeTo is Attachment1."),
    PropMeta::vector3("Torque", 1000.0, 0.0, 0.0).group("Torque").order(3).drag(10.0).desc("The torque, in the directions RelativeTo says."),
    PropMeta::enumeration("RelativeTo", "ActuatorRelativeTo", "Attachment0").group("Torque").order(4).desc("Which directions the torque is given in."),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "TorqueMover", |dm, m, part, body, _mass, _dt| {
        let torque = dm.vec3(m, "Torque");
        if !dm.flag(m, "Enabled") || torque == Vec3::ZERO {
            return;
        }
        let basis = match dm.string(m, "RelativeTo").as_str() {
            "World" => bevy::math::Quat::IDENTITY,
            "Attachment1" => mover::frame_of(dm, m, part, "Attachment1").1,
            _ => mover::frame_of(dm, m, part, "Attachment0").1,
        };
        body.apply_torque(bv(basis * torque), true);
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("TorqueMover").describe("Turns a part with a torque.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("Torque", Some("Instance"))
    .describe("A turning force on a part.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["Torque"])
    .components(&["TorqueMover"])
    .props(PROPS);