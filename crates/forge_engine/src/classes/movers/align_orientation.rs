//! AlignOrientation: turns a part so that its attachment (`Attachment0`) faces an orientation - `CFrame` (mode
//! OneAttachment) or another attachment (`Attachment1`, mode TwoAttachment) - at a speed that follows the angle
//! (`Responsiveness`, up to `MaxAngularVelocity`). With `RigidityEnabled` it turns at once.

use bevy::math::Quat;

use super::mover;
use crate::{
    core::*,
    physics::{bv, fv, StepCtx},
};

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("OrientationAlignmentMode", &["OneAttachment", "TwoAttachment"], &[0, 1])];

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("AlignOrientation").order(0).desc("Whether the part is turned."),
    PropMeta::reference("Attachment0", "Attachment").group("AlignOrientation").order(1).desc("The attachment that is turned to the orientation."),
    PropMeta::reference("Attachment1", "Attachment").group("AlignOrientation").order(2).desc("The attachment it is turned to face (TwoAttachment mode)."),
    PropMeta::enumeration("Mode", "OrientationAlignmentMode", "OneAttachment").group("AlignOrientation").order(3).desc("Whether the target is CFrame or Attachment1."),
    PropMeta::cframe("CFrame").group("AlignOrientation").order(4).desc("The orientation in the world (OneAttachment mode)."),
    PropMeta::number("MaxTorque", 10_000.0).group("AlignOrientation").order(5).drag(100.0).clamp(0.0, f32::MAX).desc("The most torque it may use (0 turns nothing)."),
    PropMeta::number("MaxAngularVelocity", 100_000.0).group("AlignOrientation").order(6).drag(1.0).clamp(0.0, f32::MAX).desc("The fastest it may turn, in radians per second."),
    PropMeta::number("Responsiveness", 10.0).group("AlignOrientation").order(7).drag(0.5).clamp(0.0, 200.0).desc("How quickly it closes the angle (per second)."),
    PropMeta::boolean("RigidityEnabled", false).group("AlignOrientation").order(8).desc("Whether it turns at once (ignoring the other limits)."),
    PropMeta::boolean("PrimaryAxisOnly", false).group("AlignOrientation").order(9).hidden(),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "AlignOrientationMover", |dm, m, part, body, _mass, dt| {
        if !dm.flag(m, "Enabled") || dm.num(m, "MaxTorque", 10_000.0) <= 0.0 {
            return;
        }
        let rotation = mover::frame_of(dm, m, part, "Attachment0").1;
        let goal = if dm.string(m, "Mode") == "TwoAttachment" {
            dm.reference(m, "Attachment1").and_then(|a| dm.world_cframe(a)).map(|f| f.1)
        } else {
            match dm.get_prop(m, "CFrame") {
                Some(Value::CF(_, q)) => Some(q),
                _ => Some(Quat::IDENTITY),
            }
        };
        let Some(goal) = goal else { return };
        let error = mover::rotation_error(rotation, goal);
        let current = fv(body.angular_velocity());
        let next = if dm.flag(m, "RigidityEnabled") {
            error / dt.max(1e-4)
        } else {
            let wanted = error * dm.num(m, "Responsiveness", 10.0);
            let fastest = dm.num(m, "MaxAngularVelocity", 100_000.0);
            if wanted.length() > fastest {
                wanted.normalize_or_zero() * fastest
            } else {
                wanted
            }
        };
        if next != current {
            body.set_angular_velocity(bv(next));
            body.set_awake(true);
        }
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("AlignOrientationMover").describe("Turns a part to an orientation.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("AlignOrientation", Some("Instance"))
    .describe("Turns a part to an orientation.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["AlignOrientation"])
    .components(&["AlignOrientationMover"])
    .props(PROPS);