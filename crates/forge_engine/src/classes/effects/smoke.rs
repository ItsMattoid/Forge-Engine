//! Smoke: smoke from a part, drawn as soft puffs that rise and grow (`render::effects`). `Size` is how big the puffs get,
//! `Opacity` how thick the smoke is, `RiseVelocity` how fast it rises (below zero it sinks) and `Color` its colour.

use bevy::math::{Vec2, Vec3};

use crate::{
    core::*,
    dm::DataModel,
    render::{
        effects::{self, Area, ColorSeq, EmitterSpec, Facing, Heading, Look, NumberSeq, Procedural},
        RenderCtx,
    },
};

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Smoke").order(0).flags(F_VISUAL).desc("Whether smoke comes out."),
    PropMeta::color3("Color", 0.77, 0.77, 0.77).group("Smoke").order(1).flags(F_VISUAL).desc("The colour of the smoke."),
    PropMeta::number("Opacity", 0.5).group("Smoke").order(2).slider(0.0, 1.0).desc("How thick the smoke is."),
    PropMeta::number("RiseVelocity", 1.0).group("Smoke").order(3).slider(-25.0, 25.0).desc("How fast the smoke rises."),
    PropMeta::number("Size", 1.0).group("Smoke").order(4).slider(0.1, 100.0).desc("How big the puffs are."),
    PropMeta::number("TimeScale", 1.0).group("Smoke").order(5).hidden().desc("Kept for scripts."),
];

/// The emitter a smoke stands for.
pub fn spec(dm: &DataModel, id: Id) -> EmitterSpec {
    let size = dm.num(id, "Size", 1.0).clamp(0.1, 100.0);
    let rise = dm.num(id, "RiseVelocity", 1.0);
    let opacity = dm.num(id, "Opacity", 0.5).clamp(0.0, 1.0);
    let colour = dm.vec3(id, "Color");
    // (The puffs have soft edges, so the middle of each is made a little thicker than the property says.)
    let peak = (opacity * 1.4).min(1.0);
    EmitterSpec {
        enabled: dm.flag(id, "Enabled"),
        rate: 9.0,
        lifetime: (3.0, 4.5),
        speed: (rise * 0.8, rise * 1.2),
        heading: Heading::World(Vec3::Y),
        spread: Vec2::splat(22.0),
        acceleration: Vec3::ZERO,
        drag: 0.25,
        size: NumberSeq::from_points(&[(0.0, size * 0.7), (1.0, size * 2.8)]),
        color: ColorSeq::from_points(&[(0.0, colour), (1.0, colour * 0.8)]),
        transparency: NumberSeq::from_points(&[(0.0, 1.0 - peak * 0.8), (0.2, 1.0 - peak), (1.0, 1.0)]),
        rotation: (0.0, 360.0),
        rot_speed: (-25.0, 25.0),
        z_offset: 0.0,
        facing: Facing::Camera,
        locked_to_part: false,
        time_scale: 1.0,
        inherit_velocity: 0.0,
        area: Area::Volume(0.3),
        look: Look::new("", Procedural::Soft, 0.0),
    }
}

fn animate(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    let Some(frame) = dm.parent(id).and_then(|p| effects::parent_frame(dm, p)) else { return ctx.fx_clear(id) };
    let spec = spec(dm, id);
    let name = dm.name(id);
    effects::run_emitter(ctx, id, &name, &spec, frame, 0.0, 0.0);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("SmokeFx").describe("Draws the puffs of the smoke (render::effects).").animate(animate);

pub static CLASS: ClassDesc =
    ClassDesc::new("Smoke", Some("Instance")).describe("Smoke from a part.").category("Effects").creatable().groups(&["Smoke"]).components(&["Animated", "SmokeFx"]).props(PROPS);
