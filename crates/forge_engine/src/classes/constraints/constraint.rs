//! Constraint: the base of everything that joins two parts - a weld, a hinge, a ball socket, a slider, a rod, a
//! rope, a spring. A constraint is an ordinary instance: set its `Part0` and `Part1`, put it in the workspace
//! (anywhere inside it) and it holds; change a property while the game runs and the joint is rebuilt;
//! disable it, clear a part or `Destroy()` it and the joint is gone. See `physics::joints`.

use bevy::math::{Quat, Vec3};

use crate::core::*;

type R = Result<ScriptValue, String>;

/// `constraint:SetWorldFrame(cframe)`: puts the joint at a place in the world by setting both frames, so that it holds
/// the parts exactly as they are now.
fn set_world_frame(c: &mut MethodCall) -> R {
    let (p, q) = c.cframe(0)?;
    if !c.dm.has_component(c.this, "JointFrames") {
        return Err(format!("{} joins its parts where they are: it has no frame to place", c.dm.class_name(c.this)));
    }
    let local = |dm: &crate::dm::DataModel, part: Option<Id>| -> (Vec3, Quat) {
        match part.and_then(|p| dm.world_cframe(p)) {
            Some((pp, pq)) => (pq.inverse() * (p - pp), (pq.inverse() * q).normalize()),
            None => (p, q),
        }
    };
    let (a, b) = (c.dm.reference(c.this, "Part0"), c.dm.reference(c.this, "Part1"));
    // (With only one part set it is the first one, as the simulation reads it.)
    let (first, second) = if a.is_none() { (b, None) } else { (a, b) };
    let (f0, f1) = (local(c.dm, first), local(c.dm, second));
    c.dm.set_prop(c.this, "C0", Value::CF(f0.0, f0.1))?;
    c.dm.set_prop(c.this, "C1", Value::CF(f1.0, f1.1))?;
    Ok(ScriptValue::Nil)
}

static METHODS: &[MethodDesc] = &[MethodDesc::new("SetWorldFrame", set_world_frame).desc("Places the joint's frame at a CFrame in the world: sets C0 and C1 so the parts are held as they are now.")];

pub static CLASS: ClassDesc = ClassDesc::new("Constraint", Some("Instance")).describe("Joins two parts.").category("Constraints").groups(&["Constraint"]).components(&["Joint"]).methods(METHODS);
