//! Rigidbody: the physical properties of a part's body - whether it is fixed, how heavy, slippery and bouncy
//! it is, how fast it moves, and how the simulation treats it. The simulation (`physics`) reads them; nothing
//! here steps anything.
//!
//! Every property is checked when it is written (numbers are kept in their ranges, a value that is not finite
//! is refused), and every one can be changed while the game runs: the simulation notices the change and applies
//! it before its next step.
//!
//! | property                   | what it does                                                                    |
//! |----------------------------|---------------------------------------------------------------------------------|
//! | `Anchored`                 | fixed in place (a static body); unanchored parts are simulated                  |
//! | `Kinematic`                | an anchored part that moves by its velocity and pushes what it meets            |
//! | `Density`                  | mass per cubic stud; `Mass` is this times the part's volume                     |
//! | `Friction`, `Elasticity`   | how much it grips (0 = ice) and bounces (0 = not at all, 1 = fully)             |
//! | `AssemblyLinearVelocity`   | its velocity in studs per second (set it to throw the part)                     |
//! | `AssemblyAngularVelocity`  | how fast it turns, in radians per second about each world axis                  |
//! | `GravityScale`             | how strongly gravity pulls it (1 = normally, 0 = floats, negative = rises)      |
//! | `LinearDamping`, `AngularDamping` | drag: the fraction of its (turning) speed it loses each second           |
//! | `CanSleep`, `Sleeping`     | whether it may be put to rest when it stops moving, and whether it is           |
//! | `ContinuousCollision`      | checks the whole path of a fast part against other moving parts (no tunneling)  |
//! | `FixedRotation`            | the simulation never turns the part (an upright body for a scripted player)     |

use crate::{core::*, physics::analytic_mass};

/// The volume of a part times its density (what the simulation gives it, for every part but a stretched ball).
fn mass(dm: &crate::dm::DataModel, id: Id) -> Option<Value> {
    Some(Value::Num(analytic_mass(dm, id)))
}

/// `CustomPhysicalProperties`: Roblox's way to set density, friction and elasticity together. Setting it sets those three;
/// reading it gives them back while it is set (nil otherwise: the part has the engine's defaults).
fn get_custom_physics(dm: &crate::dm::DataModel, id: Id) -> Option<Value> {
    let numbers = if dm.flag(id, "CustomPhysics") { vec![dm.num(id, "Density", 1.0), dm.num(id, "Friction", 0.3), dm.num(id, "Elasticity", 0.0), 1.0, 1.0] } else { Vec::new() };
    Some(Value::Data("PhysicalProperties", numbers))
}

fn set_custom_physics(dm: &mut crate::dm::DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::Data(_, n) = v else { return Err("CustomPhysicalProperties must be a PhysicalProperties or nil".into()) };
    if n.is_empty() {
        dm.set_prop(id, "CustomPhysics", Value::Bool(false))?;
        dm.set_prop(id, "Density", Value::Num(1.0))?;
        dm.set_prop(id, "Friction", Value::Num(0.3))?;
        return dm.set_prop(id, "Elasticity", Value::Num(0.0));
    }
    dm.set_prop(id, "Density", Value::Num(n[0]))?;
    dm.set_prop(id, "Friction", Value::Num(n.get(1).copied().unwrap_or(0.3)))?;
    dm.set_prop(id, "Elasticity", Value::Num(n.get(2).copied().unwrap_or(0.0)))?;
    dm.set_prop(id, "CustomPhysics", Value::Bool(true))
}

fn get_assembly_mass(dm: &crate::dm::DataModel, id: Id) -> Option<Value> {
    Some(Value::Num(analytic_mass(dm, id)))
}

fn get_assembly_centre(dm: &crate::dm::DataModel, id: Id) -> Option<Value> {
    dm.world_cframe(id).map(|(p, _)| Value::V3(p))
}

/// Every part is its own assembly (welded groups are not tracked), so its root is itself.
fn get_assembly_root(_dm: &crate::dm::DataModel, id: Id) -> Option<Value> {
    Some(Value::Ref(Some(id)))
}

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("CustomPhysics", false).hidden().desc("Whether CustomPhysicalProperties is set."),
    PropMeta::data("CustomPhysicalProperties", "PhysicalProperties", &[])
        .group("Physics")
        .order(14)
        .hidden()
        .no_save()
        .desc("Density, friction and elasticity as one value (nil: the defaults).")
        .computed(get_custom_physics, set_custom_physics),
    PropMeta::boolean("Massless", false).group("Physics").order(15).hidden().flags(F_PHYS).desc("Whether the part adds no mass to the group of parts it is welded to (welded groups are not tracked: kept for scripts)."),
    PropMeta::number("AssemblyMass", 0.0).group("Physics").order(16).no_save().hidden().computed_ro(get_assembly_mass).desc("The mass of the group of parts joined to this one (a part is its own group: its own mass)."),
    PropMeta::vector3("AssemblyCenterOfMass", 0.0, 0.0, 0.0).group("Physics").order(17).no_save().hidden().computed_ro(get_assembly_centre).desc("The centre of mass of the group of parts joined to this one, in the world."),
    PropMeta::vector3("CenterOfMass", 0.0, 0.0, 0.0).group("Physics").order(18).hidden().readonly().desc("The centre of mass of the part, relative to the part (its middle)."),
    PropMeta::reference("AssemblyRootPart", "BasePart").group("Physics").order(19).no_save().hidden().computed_ro(get_assembly_root).desc("The part that stands for the group of parts joined to this one."),
    PropMeta::integer("RootPriority", 0).group("Physics").order(20).hidden().desc("Which part of a welded group is its root (kept for scripts)."),
    PropMeta::boolean("EnableFluidForces", true).group("Physics").order(21).hidden().desc("Whether the part feels air and water (kept for scripts)."),
    PropMeta::boolean("Anchored", false).group("Physics").order(0).flags(F_PHYS).desc("Anchored parts never move on their own; unanchored ones are moved by gravity and by whatever hits them."),
    PropMeta::boolean("Kinematic", false)
        .group("Physics")
        .order(1)
        .flags(F_PHYS)
        .when_true("Anchored")
        .desc("An anchored part that moves by its AssemblyLinearVelocity / AssemblyAngularVelocity (or toward a CFrame a script sets) and pushes unanchored parts, without being pushed in return."),
    PropMeta::number("Density", 1.0).group("Physics").order(2).flags(F_PHYS).drag(0.05).clamp(0.001, 10_000.0).desc("Mass per cubic stud. A part's Mass is its volume times this."),
    PropMeta::number("Mass", 0.0).group("Physics").order(3).computed_ro(mass).desc("How much matter the part has: its volume times its Density. Read-only."),
    PropMeta::number("Friction", 0.3)
        .group("Physics")
        .order(4)
        .flags(F_PHYS)
        .slider(0.0, 2.0)
        .desc("How well the part grips what it touches (0 = ice). Two parts grip as the square root of the product of their frictions."),
    PropMeta::number("Elasticity", 0.0)
        .group("Physics")
        .order(5)
        .flags(F_PHYS)
        .slider(0.0, 1.0)
        .desc("How much of its speed the part keeps when it hits something (0 = none, 1 = all). Two parts bounce as the larger of their elasticities."),
    PropMeta::vector3("AssemblyLinearVelocity", 0.0, 0.0, 0.0)
        .group("Physics")
        .order(6)
        .flags(F_PHYS)
        .runtime()
        .drag(0.5)
        .aliases(&["Velocity"])
        .desc("The part's velocity in studs per second. Set it to throw the part. (Ignored while the part is anchored, unless it is also Kinematic.)"),
    PropMeta::vector3("AssemblyAngularVelocity", 0.0, 0.0, 0.0)
        .group("Physics")
        .order(7)
        .flags(F_PHYS)
        .runtime()
        .drag(0.05)
        .aliases(&["RotVelocity"])
        .desc("How fast the part turns, in radians per second about each world axis. (Ignored while the part is anchored, unless it is also Kinematic.)"),
    PropMeta::number("GravityScale", 1.0)
        .group("Physics")
        .order(8)
        .flags(F_PHYS)
        .drag(0.05)
        .clamp(-100.0, 100.0)
        .desc("How strongly gravity pulls the part: 1 is normally, 0 lets it float, a negative number makes it rise."),
    PropMeta::number("LinearDamping", 0.0).group("Physics").order(9).flags(F_PHYS).drag(0.01).clamp(0.0, 1000.0).desc("Drag on the part's movement: the larger, the faster it slows down in the air."),
    PropMeta::number("AngularDamping", 0.0).group("Physics").order(10).flags(F_PHYS).drag(0.01).clamp(0.0, 1000.0).desc("Drag on the part's turning: the larger, the faster it stops spinning."),
    PropMeta::boolean("CanSleep", true)
        .group("Physics")
        .order(11)
        .flags(F_PHYS)
        .desc("Whether the simulation may put the part to rest once it has stopped moving (it wakes when something touches it). Resting parts cost nothing."),
    PropMeta::boolean("Sleeping", false).group("Physics").order(12).runtime().readonly().desc("Whether the part is at rest right now."),
    PropMeta::boolean("FixedRotation", false)
        .group("Physics")
        .order(22)
        .flags(F_PHYS)
        .desc("Stops the simulation from turning the part (collisions cannot tip it over): what an upright player body, a lift or a rolling door wants. Scripts and the editor can still rotate it."),
    PropMeta::boolean("ContinuousCollision", false)
        .group("Physics")
        .order(13)
        .flags(F_PHYS)
        .desc("Checks the whole path of a fast part against other moving parts, so it cannot pass through a thin one between two steps. (Every part is always checked against anchored parts.)"),
];

pub static COMPONENT: ComponentDesc = ComponentDesc::new("Rigidbody").describe("The physical properties of a part's body.").requires(&["Transform"]).props(PROPS);
