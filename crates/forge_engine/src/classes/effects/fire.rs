//! Fire: flames on a part, drawn as glowing puffs that rise (`render::effects`). `Size` is how big the fire is (the puffs and
//! how many of them), `Heat` how fast the flames rise (below zero they sink), `Color` the colour of the flames and
//! `SecondaryColor` the colour of the glowing heart of them. The flames start inside the part and are added to the light
//! behind them, so they glow with Bloom. A fire does not light its surroundings (add a light to the part for that).

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
    PropMeta::boolean("Enabled", true).group("Fire").order(0).flags(F_VISUAL).desc("Whether the fire burns."),
    PropMeta::color3("Color", 1.0, 0.0, 0.0).group("Fire").order(1).flags(F_VISUAL).desc("The colour of the flames."),
    PropMeta::color3("SecondaryColor", 0.93, 0.53, 0.0).group("Fire").order(2).flags(F_VISUAL).desc("The colour of the heart of the flames."),
    PropMeta::number("Heat", 9.0).group("Fire").order(3).slider(-25.0, 25.0).desc("How fast the flames rise."),
    PropMeta::number("Size", 5.0).group("Fire").order(4).slider(2.0, 30.0).desc("How big the fire is."),
    PropMeta::number("TimeScale", 1.0).group("Fire").order(5).slider(0.0, 1.0).hidden().desc("Kept for scripts."),
];

/// The emitter a fire stands for.
pub fn spec(dm: &DataModel, id: Id) -> EmitterSpec {
    let size = dm.num(id, "Size", 5.0).clamp(0.5, 60.0);
    let heat = dm.num(id, "Heat", 9.0);
    let (outer, heart) = (dm.vec3(id, "Color"), dm.vec3(id, "SecondaryColor"));
    let puff = size * 0.6;
    EmitterSpec {
        enabled: dm.flag(id, "Enabled"),
        rate: 18.0 + size * 3.0,
        lifetime: (0.7, 1.2),
        speed: (heat * 0.35, heat * 0.55),
        heading: Heading::World(Vec3::Y),
        spread: Vec2::splat(14.0),
        acceleration: Vec3::ZERO,
        drag: 0.6,
        size: NumberSeq::from_points(&[(0.0, puff * 0.7), (0.3, puff), (1.0, puff * 0.1)]),
        color: ColorSeq::from_points(&[(0.0, heart.lerp(Vec3::ONE, 0.45)), (0.35, heart), (0.75, outer), (1.0, outer * 0.35)]),
        transparency: NumberSeq::from_points(&[(0.0, 0.15), (0.6, 0.45), (1.0, 1.0)]),
        rotation: (0.0, 360.0),
        rot_speed: (-45.0, 45.0),
        z_offset: 0.0,
        facing: Facing::Camera,
        locked_to_part: false,
        time_scale: 1.0,
        inherit_velocity: 0.0,
        area: Area::Volume(0.35),
        look: Look::new("", Procedural::Soft, 1.0),
    }
}

fn animate(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    let Some(frame) = dm.parent(id).and_then(|p| effects::parent_frame(dm, p)) else { return ctx.fx_clear(id) };
    let spec = spec(dm, id);
    let name = dm.name(id);
    effects::run_emitter(ctx, id, &name, &spec, frame, 0.0, 0.0);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("FireFx").describe("Draws the flames of the fire (render::effects).").animate(animate);

pub static CLASS: ClassDesc = ClassDesc::new("Fire", Some("Instance")).describe("Flames on a part.").category("Effects").creatable().groups(&["Fire"]).components(&["Animated", "FireFx"]).props(PROPS);
