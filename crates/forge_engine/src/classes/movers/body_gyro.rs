//! BodyGyro: turns a part toward an orientation (`CFrame`) like a damped spring - `P` is how stiff, `D` how much it
//! resists turning - using at most `MaxTorque` (per axis).
use bevy::math::{Quat, Vec3};

use super::mover;
use crate::{
    core::*,
    physics::{bv, fv, StepCtx},
};
static PROPS: &[PropMeta] = &[
    PropMeta::cframe("CFrame").group("BodyGyro").order(0).desc("The orientation the part is turned to (its position is ignored)."),
    PropMeta::vector3("MaxTorque", 400_000.0, 400_000.0, 400_000.0).group("BodyGyro").order(1).drag(1000.0).desc("The most torque it may use on each axis (0 leaves the axis alone)."),
    PropMeta::number("P", 3_000.0).group("BodyGyro").order(2).drag(10.0).clamp(0.0, 1.0e9).desc("How stiff the turning is."),
    PropMeta::number("D", 500.0).group("BodyGyro").order(3).drag(10.0).clamp(0.0, 1.0e9).desc("How much the part resists turning (damping)."),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "BodyGyroMover", |dm, m, part, body, _mass, dt| {
        let (stiffness, damping) = (dm.num(m, "P", 3_000.0), dm.num(m, "D", 500.0));
        let Some((_, rotation)) = dm.world_cframe(part) else { return };
        let target = match dm.get_prop(m, "CFrame") {
            Some(Value::CF(_, q)) => q,
            _ => Quat::IDENTITY,
        };
        let torque = dm.vec3(m, "MaxTorque");
        let error = mover::rotation_error(rotation, target);
        let current = fv(body.angular_velocity());
        let wanted = (current + error * stiffness * dt) / (1.0 + damping * dt);
        // (The torque limit decides which axes are driven; the body's inertia is not asked, so it is a switch, not a bound.)
        let next = Vec3::new(if torque.x > 0.0 { wanted.x } else { current.x }, if torque.y > 0.0 { wanted.y } else { current.y }, if torque.z > 0.0 { wanted.z } else { current.z });
        if next != current {
            body.set_angular_velocity(bv(next));
            body.set_awake(true);
        }
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("BodyGyroMover").describe("Turns a part toward an orientation.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("BodyGyro", Some("Instance"))
    .describe("Turns a part toward an orientation.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["BodyGyro"])
    .components(&["BodyGyroMover"])
    .props(PROPS);