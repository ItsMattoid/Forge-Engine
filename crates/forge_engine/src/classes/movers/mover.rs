//! What the movers share. A *mover* is an object in a part (`BodyVelocity`, `BodyForce`, `LinearVelocity`,
//! `VectorForce`, `AlignPosition` ...) that pushes the part around: each step, before the world moves, it reads its
//! settings and gives the part's body a push. A mover works on the part it is in (or, for the newer ones, the part its
//! `Attachment0` is in), and only on parts that are not anchored.
//!
//! This file holds the arithmetic and the loop each mover's `step` is built from: `for_each` visits the movers of a kind
//! with their part's body, and `approach` / `rotation_error` are the pieces of "move toward a target, using at most so much
//! force". (Forces are in mass x studs / second^2, the engine's units.)

use bevy::math::{Quat, Vec3};
use box3d::Body;

use crate::{core::Id, dm::DataModel, physics::StepCtx};

/// The part a mover pushes: the part `Attachment0` is in when it is set, else the part the mover is in.
pub fn part_of(dm: &DataModel, mover: Id) -> Option<Id> {
    let host = dm.reference(mover, "Attachment0").and_then(|a| dm.parent(a)).or_else(|| dm.parent(mover))?;
    (dm.is_a(host, "BasePart") && dm.in_workspace(mover)).then_some(host)
}

/// The world pose a mover pushes relative to: its attachment's, else its part's.
pub fn frame_of(dm: &DataModel, mover: Id, part: Id, attachment_prop: &str) -> (Vec3, Quat) {
    dm.reference(mover, attachment_prop).and_then(|a| dm.world_cframe(a)).or_else(|| dm.world_cframe(part)).unwrap_or((Vec3::ZERO, Quat::IDENTITY))
}

/// Calls `f(dm, mover, part, body, mass)` for every mover of a kind (a component) whose part is simulated and can move.
pub fn for_each(ctx: &mut StepCtx, component: &str, mut f: impl FnMut(&mut DataModel, Id, Id, &Body<'static>, f32, f32)) {
    for mover in ctx.dm.with_component(component) {
        let Some(part) = part_of(ctx.dm, mover) else { continue };
        let Some(body) = ctx.bridge.mover_body(part) else { continue };
        let mass = body.mass();
        f(ctx.dm, mover, part, body, mass, ctx.dt);
    }
}

/// `v` moved toward `target` by at most `max_force / mass` per second, axis by axis (`max_force` of 0 on an axis leaves it
/// alone; infinity jumps to the target).
pub fn approach(v: Vec3, target: Vec3, max_force: Vec3, mass: f32, dt: f32) -> Vec3 {
    let step = |current: f32, goal: f32, limit: f32| {
        if limit <= 0.0 || limit.is_nan() {
            current
        } else if limit.is_infinite() {
            goal
        } else {
            let allowed = limit / mass.max(1e-6) * dt;
            current + (goal - current).clamp(-allowed, allowed)
        }
    };
    Vec3::new(step(v.x, target.x, max_force.x), step(v.y, target.y, max_force.y), step(v.z, target.z, max_force.z))
}

/// Like [`approach`] with one force limit for the whole vector (its length): the speed changes along the line to the target.
pub fn approach_total(v: Vec3, target: Vec3, max_force: f32, mass: f32, dt: f32) -> Vec3 {
    if max_force <= 0.0 || max_force.is_nan() {
        return v;
    }
    if max_force.is_infinite() {
        return target;
    }
    let allowed = max_force / mass.max(1e-6) * dt;
    let delta = target - v;
    let length = delta.length();
    if length <= allowed {
        target
    } else {
        v + delta / length * allowed
    }
}

/// The turn (axis x angle, in radians) that takes rotation `from` to rotation `to` by the short way.
pub fn rotation_error(from: Quat, to: Quat) -> Vec3 {
    let mut delta = (to * from.inverse()).normalize();
    if delta.w < 0.0 {
        delta = -delta;
    }
    let (axis, angle) = delta.to_axis_angle();
    if angle.is_finite() {
        axis * angle
    } else {
        Vec3::ZERO
    }
}
