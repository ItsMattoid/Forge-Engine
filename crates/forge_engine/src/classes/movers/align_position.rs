//! AlignPosition: moves a part so that its attachment (`Attachment0`) is at a position - a point in the world
//! (`Position`, mode OneAttachment) or another attachment (`Attachment1`, mode TwoAttachment) - at a speed that follows the
//! distance (`Responsiveness`, up to `MaxVelocity`) using at most `MaxForce`. With `RigidityEnabled` it gets there at once.

use super::mover;
use crate::{
    core::*,
    physics::{bv, fv, StepCtx},
};

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("PositionAlignmentMode", &["OneAttachment", "TwoAttachment"], &[0, 1])];

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("AlignPosition").order(0).desc("Whether the part is moved."),
    PropMeta::reference("Attachment0", "Attachment").group("AlignPosition").order(1).desc("The attachment that is brought to the position."),
    PropMeta::reference("Attachment1", "Attachment").group("AlignPosition").order(2).desc("The attachment it is brought to (TwoAttachment mode)."),
    PropMeta::enumeration("Mode", "PositionAlignmentMode", "OneAttachment").group("AlignPosition").order(3).desc("Whether the target is Position or Attachment1."),
    PropMeta::vector3("Position", 0.0, 0.0, 0.0).group("AlignPosition").order(4).drag(0.5).desc("The point in the world (OneAttachment mode)."),
    PropMeta::number("MaxForce", 10_000.0).group("AlignPosition").order(5).drag(100.0).clamp(0.0, f32::MAX).desc("The most force it may use."),
    PropMeta::number("MaxVelocity", 10_000.0).group("AlignPosition").order(6).drag(1.0).clamp(0.0, f32::MAX).desc("The fastest it may move, in studs per second."),
    PropMeta::number("Responsiveness", 10.0).group("AlignPosition").order(7).drag(0.5).clamp(0.0, 200.0).desc("How quickly it closes the distance (per second)."),
    PropMeta::boolean("RigidityEnabled", false).group("AlignPosition").order(8).desc("Whether it gets there at once (ignoring the other limits)."),
    PropMeta::boolean("ApplyAtCenterOfMass", false).group("AlignPosition").order(9).hidden(),
];

fn step(ctx: &mut StepCtx) {
    mover::for_each(ctx, "AlignPositionMover", |dm, m, part, body, mass, dt| {
        if !dm.flag(m, "Enabled") {
            return;
        }
        let here = mover::frame_of(dm, m, part, "Attachment0").0;
        let goal = if dm.string(m, "Mode") == "TwoAttachment" { dm.reference(m, "Attachment1").and_then(|a| dm.world_cframe(a)).map(|f| f.0) } else { Some(dm.vec3(m, "Position")) };
        let Some(goal) = goal else { return };
        let current = fv(body.linear_velocity());
        let error = goal - here;
        let next = if dm.flag(m, "RigidityEnabled") {
            // Close the whole distance in this step.
            error / dt.max(1e-4)
        } else {
            let wanted = error * dm.num(m, "Responsiveness", 10.0);
            let fastest = dm.num(m, "MaxVelocity", 10_000.0);
            let wanted = if wanted.length() > fastest { wanted.normalize_or_zero() * fastest } else { wanted };
            mover::approach_total(current, wanted, dm.num(m, "MaxForce", 10_000.0), mass, dt)
        };
        if next != current {
            body.set_linear_velocity(bv(next));
            body.set_awake(true);
        }
    });
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("AlignPositionMover").describe("Brings a part to a position.").physics(30, step);

pub static CLASS: ClassDesc = ClassDesc::new("AlignPosition", Some("Instance"))
    .describe("Brings a part to a position.")
    .category("Movers")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a mover pushes the part it is in." })
    .groups(&["AlignPosition"])
    .components(&["AlignPositionMover"])
    .props(PROPS);