//! LinearVelocity: drives a part toward a velocity, using at most `MaxForce` in total. (Only the Vector mode drives all
//! three axes; Line and Plane modes are stored but behave as Vector.)

use super::mover;
use crate::{
    core::*,
    physics::{bv, fv, StepCtx},
};

pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::numbered("VelocityConstraintMode", &["Line", "Plane", "Vector"], &[0, 1, 2]),
    EnumDesc::numbered("ForceLimitMode", &["Magnitude", "PerAxis"], &[0, 1]),
];

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("LinearVelocity").order(0).desc("Whether the part is driven."),
    PropMeta::reference("Attachment0", "Attachment").group("LinearVelocity").order(1).desc("Whose part is driven."),
    PropMeta::vector3("VectorVelocity", 0.0, 0.0, 0.0).group("LinearVelocity").order(2).drag(0.5).desc("The velocity the part is driven toward, in studs per second."),
    PropMeta::number("MaxForce", 1000.0).group("LinearVelocity").order(3).drag(100.0).clamp(0.0, f32::MAX).desc("The most force it may use."),
    PropMeta::enumeration("RelativeTo", "ActuatorRelativeTo", "World").group("LinearVelocity").order(4).desc("Which directions the velocity is given in."),
    PropMeta::enumeration("VelocityConstraintMode", "VelocityConstraintMode", "Vector").group("LinearVelocity").order(5).desc("Which axes are driven (only Vector is implemented)."),
    PropMeta::enumeration("ForceLimitMode", "ForceLimitMode", "Magnitude").group("LinearVelocity").order(6).hidden(),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "LinearVelocityMover", |dm, m, part, body, mass, dt| {
        if !dm.flag(m, "Enabled") {
            return;
        }
        let basis = match dm.string(m, "RelativeTo").as_str() {
            "Attachment0" => mover::frame_of(dm, m, part, "Attachment0").1,
            _ => bevy::math::Quat::IDENTITY,
        };
        let target = basis * dm.vec3(m, "VectorVelocity");
        let current = fv(body.linear_velocity());
        let next = mover::approach_total(current, target, dm.num(m, "MaxForce", 1000.0), mass, dt);
        if next != current {
            body.set_linear_velocity(bv(next));
            body.set_awake(true);
        }
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("LinearVelocityMover").describe("Drives a part toward a velocity.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("LinearVelocity", Some("Instance"))
    .describe("Drives a part toward a velocity.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["LinearVelocity"])
    .components(&["LinearVelocityMover"])
    .props(PROPS);