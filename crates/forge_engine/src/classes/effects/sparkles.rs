//! Sparkles: glints that flicker in and out around a part, drawn as small stars that add their light to what is behind them
//! (`render::effects`). `SparkleColor` is their colour. How many there are follows the size of the part.

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
    PropMeta::boolean("Enabled", true).group("Sparkles").order(0).flags(F_VISUAL).desc("Whether the part sparkles."),
    PropMeta::color3("SparkleColor", 0.56, 0.3, 0.9).group("Sparkles").order(1).flags(F_VISUAL).desc("The colour of the sparkles."),
    PropMeta::number("TimeScale", 1.0).group("Sparkles").order(2).slider(0.0, 1.0).hidden().desc("Kept for scripts."),
];

/// The emitter a sparkles object stands for, on a part of this size.
pub fn spec(dm: &DataModel, id: Id, part_size: Vec3) -> EmitterSpec {
    let colour = dm.vec3(id, "SparkleColor");
    let surface = 2.0 * (part_size.x * part_size.y + part_size.y * part_size.z + part_size.x * part_size.z);
    let glint = (part_size.max_element() * 0.22).clamp(0.45, 2.5);
    EmitterSpec {
        enabled: dm.flag(id, "Enabled"),
        rate: (surface * 0.8).clamp(8.0, 80.0),
        lifetime: (0.4, 1.0),
        speed: (0.0, 1.2),
        heading: Heading::World(Vec3::Y),
        spread: Vec2::splat(180.0),
        acceleration: Vec3::ZERO,
        drag: 1.0,
        size: NumberSeq::from_points(&[(0.0, 0.0), (0.25, glint), (1.0, 0.0)]),
        color: ColorSeq::from_points(&[(0.0, Vec3::ONE), (0.3, colour.lerp(Vec3::ONE, 0.5)), (1.0, colour)]),
        transparency: NumberSeq::constant(0.0),
        rotation: (0.0, 90.0),
        rot_speed: (-90.0, 90.0),
        z_offset: 0.0,
        facing: Facing::Camera,
        locked_to_part: true,
        time_scale: 1.0,
        inherit_velocity: 0.0,
        area: Area::Surface(glint),
        look: Look::new("", Procedural::Star, 1.0),
    }
}

fn animate(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    let Some(frame) = dm.parent(id).and_then(|p| effects::parent_frame(dm, p)) else { return ctx.fx_clear(id) };
    let spec = spec(dm, id, frame.size);
    let name = dm.name(id);
    effects::run_emitter(ctx, id, &name, &spec, frame, 0.0, 0.0);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("SparklesFx").describe("Draws the glints of the sparkles (render::effects).").animate(animate);

pub static CLASS: ClassDesc =
    ClassDesc::new("Sparkles", Some("Instance")).describe("Sparkles on a part.").category("Effects").creatable().groups(&["Sparkles"]).components(&["Animated", "SparklesFx"]).props(PROPS);
