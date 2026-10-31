//! Explosion: a blast that pushes the parts around it. Put it in the Workspace (with a `Position`) and every part
//! within `BlastRadius` that is not anchored is thrown away from the centre; `Hit(part, distance)` fires for each part in
//! range, anchored or not. The blast happens once, when the explosion enters the Workspace, and (with `Visible`) shows as a
//! burst of fire that spreads about `BlastRadius` studs and fades in under a second (`render::effects`); there are no craters
//! (`ExplosionType` is kept for scripts).

use bevy::math::{Quat, Vec2, Vec3};

use crate::{
    core::*,
    dm::DataModel,
    render::{
        effects::{self, Area, ColorSeq, EmitterSpec, Facing, Frame, Heading, Look, NumberSeq, Procedural},
        RenderCtx,
    },
};

pub static ENUMS: &[EnumDesc] = &[EnumDesc::new("ExplosionType", &["NoCraters", "Craters"])];

static PROPS: &[PropMeta] = &[
    PropMeta::vector3("Position", 0.0, 0.0, 0.0).group("Explosion").order(0).drag(0.1).desc("Where the blast is centred, in the world."),
    PropMeta::number("BlastRadius", 4.0).group("Explosion").order(1).drag(0.1).clamp(0.0, 100.0).desc("How far the blast reaches, in studs."),
    PropMeta::number("BlastPressure", 500_000.0).group("Explosion").order(2).drag(1000.0).clamp(0.0, 1.0e9).desc("How hard the blast pushes."),
    PropMeta::number("DestroyJointRadiusPercent", 1.0).group("Explosion").order(3).slider(0.0, 1.0).desc("How much of the radius breaks joints (the constraints of parts in it are destroyed)."),
    PropMeta::enumeration("ExplosionType", "ExplosionType", "Craters").group("Explosion").order(4).hidden().desc("Whether it digs craters (kept for scripts: there is no terrain to dig)."),
    PropMeta::boolean("Visible", true).group("Explosion").order(5).desc("Whether the fireball is drawn."),
    PropMeta::boolean("Exploded", false).runtime().hidden().desc("Whether the blast already happened."),
];

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("Hit").desc("A part was in range of the blast; passes (part, distance from the centre).")];

/// The speed a part of this mass gains: the pressure pushes, a heavier part moves less, nearer parts move more. (At the
/// default pressure a small part is thrown several tens of studs per second; a part a hundred times heavier barely stirs.)
fn push_speed(pressure: f32, mass: f32, falloff: f32) -> f32 {
    (pressure * 1.3e-3 / mass.max(0.05) * falloff).min(250.0)
}

fn detonate(dm: &mut DataModel, id: Id) {
    if dm.flag(id, "Exploded") {
        return;
    }
    let _ = dm.set_prop(id, "Exploded", Value::Bool(true));
    let (centre, radius, pressure) = (dm.vec3(id, "Position"), dm.num(id, "BlastRadius", 4.0), dm.num(id, "BlastPressure", 500_000.0));
    let breaking = radius * dm.num(id, "DestroyJointRadiusPercent", 1.0);
    let parts: Vec<Id> = dm.subtree(dm.workspace).into_iter().filter(|n| dm.is_a(*n, "BasePart")).collect();
    for part in parts {
        let Some((position, _)) = dm.world_cframe(part) else { continue };
        let distance = position.distance(centre);
        // A part is in range when any of it is: the nearest point of its box is within the radius.
        let nearest = (distance - dm.size(part).max_element() * 0.5).max(0.0);
        if nearest > radius {
            continue;
        }
        if !dm.flag(part, "Anchored") {
            let away = (position - centre).try_normalize().unwrap_or(Vec3::Y);
            let mass = dm.num(part, "Mass", 1.0);
            let falloff = 1.0 - (nearest / radius.max(0.01)).clamp(0.0, 1.0) * 0.7;
            let velocity = dm.vec3(part, "AssemblyLinearVelocity") + away * push_speed(pressure, mass, falloff);
            let _ = dm.set_prop(part, "AssemblyLinearVelocity", Value::V3(velocity));
        }
        if nearest <= breaking {
            for joint in dm.with_component("Joint") {
                if dm.reference(joint, "Part0") == Some(part) || dm.reference(joint, "Part1") == Some(part) {
                    dm.destroy(joint);
                }
            }
        }
        dm.fire(id, "Hit", || vec![ScriptValue::Instance(part), ScriptValue::Num(f64::from(distance))]);
    }
}

/// How many puffs the fireball is made of.
const FIREBALL_PUFFS: f32 = 70.0;

/// The fireball of a blast of this radius: a burst of glowing puffs that fly out, slow down, swell and fade.
pub fn fireball(radius: f32) -> EmitterSpec {
    let r = radius.max(0.5);
    EmitterSpec {
        enabled: false,
        rate: 0.0,
        lifetime: (0.35, 0.8),
        speed: (r * 0.6, r * 2.2),
        heading: Heading::World(Vec3::Y),
        spread: Vec2::splat(180.0),
        acceleration: Vec3::new(0.0, r * 0.5, 0.0),
        drag: 3.0,
        size: NumberSeq::from_points(&[(0.0, r * 0.5), (0.4, r * 0.9), (1.0, r * 1.2)]),
        color: ColorSeq::from_points(&[(0.0, Vec3::new(1.0, 0.95, 0.7)), (0.3, Vec3::new(1.0, 0.6, 0.15)), (1.0, Vec3::new(0.35, 0.1, 0.05))]),
        transparency: NumberSeq::from_points(&[(0.0, 0.1), (0.5, 0.4), (1.0, 1.0)]),
        rotation: (0.0, 360.0),
        rot_speed: (-90.0, 90.0),
        z_offset: 0.0,
        facing: Facing::Camera,
        locked_to_part: false,
        time_scale: 1.0,
        inherit_velocity: 0.0,
        area: Area::Point,
        look: Look::new("", Procedural::Soft, 1.0),
    }
}

fn animate(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    if !(dm.flag(id, "Visible") && dm.flag(id, "Exploded") && dm.in_workspace(id)) {
        return ctx.fx_clear(id);
    }
    let frame = Frame { position: dm.vec3(id, "Position"), rotation: Quat::IDENTITY, size: Vec3::ZERO };
    let name = dm.name(id);
    effects::run_emitter(ctx, id, &name, &fireball(dm.num(id, "BlastRadius", 4.0)), frame, FIREBALL_PUFFS, 0.0);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("ExplosionFx").describe("Draws the fireball of the explosion (render::effects).").animate(animate);

struct ExplosionBehavior;

impl ClassBehavior for ExplosionBehavior {
    fn on_parent_changed(&self, dm: &mut DataModel, id: Id, _old: Option<Id>, new: Option<Id>) {
        if new.is_some() && dm.in_workspace(id) {
            detonate(dm, id);
        }
    }
}

pub static CLASS: ClassDesc = ClassDesc::new("Explosion", Some("Instance"))
    .describe("A blast that pushes the parts around it.")
    .category("Effects")
    .creatable()
    .parent_rule(ParentRule::Service("Workspace"))
    .groups(&["Explosion"])
    .components(&["Animated", "ExplosionFx"])
    .props(PROPS)
    .signals(SIGNALS)
    .behavior(&ExplosionBehavior);