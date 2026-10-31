//! ParticleEmitter: puffs of particles from a part or an attachment, drawn as quads that face the camera (`render::effects`).
//!
//! `Rate` particles a second are born (while `Enabled`) on the side of the part `EmissionDirection` names, or at the
//! attachment; each lives `Lifetime` seconds, leaves at `Speed` studs a second within `SpreadAngle` degrees of the direction,
//! is pushed by `Acceleration` and slowed by `Drag`, and changes `Size`, `Color` and `Transparency` over its life. `Emit(n)`
//! makes n at once whatever `Enabled` says, `Clear()` removes them all. `Texture` is a project picture (a soft round puff
//! when there is none). A `LightEmission` of half or more adds the light of the particles to what is behind them (fire, magic);
//! less blends them over it (smoke, dust). `LockedToPart` makes particles move with the part. `VelocityInheritance` gives them
//! part of the speed the part is moving at.
//!
//! Particles are not shaded by the world's lights and `Shape`, `ShapeStyle` and `ShapeInOut` are not used: particles always
//! come from the face (those properties are kept for scripts and hidden).

use crate::{
    classes::world::decal::face_axes,
    core::*,
    dm::DataModel,
    render::{
        effects::{self, Area, EmitterSpec, Facing, Heading, Look, Procedural},
        RenderCtx,
    },
};

pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::new("ParticleOrientation", &["FacingCamera", "FacingCameraWorldUp", "VelocityParallel", "VelocityPerpendicular"]),
    EnumDesc::new("ParticleEmitterShape", &["Box", "Sphere", "Cylinder", "Disc"]),
    EnumDesc::new("ParticleEmitterShapeStyle", &["Volume", "Surface"]),
    EnumDesc::new("ParticleEmitterShapeInOut", &["Outward", "Inward", "InAndOut"]),
];

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Emission").order(0).flags(F_VISUAL).desc("Whether particles are emitted."),
    PropMeta::number("Rate", 20.0).group("Emission").order(1).drag(0.5).clamp(0.0, 20_000.0).desc("Particles per second."),
    PropMeta::data("Lifetime", "NumberRange", &[5.0, 10.0]).group("Emission").order(2).desc("How long a particle lives, in seconds (random between the two)."),
    PropMeta::data("Speed", "NumberRange", &[5.0, 5.0]).group("Emission").order(3).desc("How fast a particle leaves, in studs per second."),
    PropMeta::enumeration("EmissionDirection", "NormalId", "Top").group("Emission").order(4).desc("Which side of the part particles leave from."),
    PropMeta::vector2("SpreadAngle", 0.0, 0.0).group("Emission").order(5).desc("How far from the direction particles may go, in degrees."),
    PropMeta::vector3("Acceleration", 0.0, 0.0, 0.0).group("Emission").order(6).drag(0.1).desc("A constant push on every particle."),
    PropMeta::number("Drag", 0.0).group("Emission").order(7).drag(0.05).clamp(0.0, 1000.0).desc("How quickly particles slow down."),
    PropMeta::data("Size", "NumberSequence", &[0.0, 1.0, 0.0, 1.0, 1.0, 0.0]).group("Appearance").order(0).desc("How big a particle is over its life."),
    PropMeta::data("Color", "ColorSequence", &[0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]).group("Appearance").order(1).desc("The colour of a particle over its life."),
    PropMeta::data("Transparency", "NumberSequence", &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]).group("Appearance").order(2).desc("How see-through a particle is over its life."),
    PropMeta::asset("Texture", "png").group("Appearance").order(3).desc("The picture of a particle (a soft round puff when there is none)."),
    PropMeta::data("Rotation", "NumberRange", &[0.0, 0.0]).group("Appearance").order(4).desc("The angle a particle starts at, in degrees."),
    PropMeta::data("RotSpeed", "NumberRange", &[0.0, 0.0]).group("Appearance").order(5).desc("How fast a particle turns, in degrees per second."),
    PropMeta::number("LightEmission", 0.0).group("Appearance").order(6).slider(0.0, 1.0).desc("How much the particles glow: from half, their light is added to what is behind them."),
    PropMeta::number("LightInfluence", 0.0).group("Appearance").order(7).slider(0.0, 1.0).hidden().desc("Kept for scripts: particles are not shaded by the world's lights."),
    PropMeta::number("ZOffset", 0.0).group("Appearance").order(8).drag(0.1).desc("Moves the particles toward or away from the camera."),
    PropMeta::enumeration("Orientation", "ParticleOrientation", "FacingCamera").group("Appearance").order(9).desc("How particles face."),
    PropMeta::boolean("LockedToPart", false).group("Emission").order(8).desc("Whether particles move with the part."),
    PropMeta::number("TimeScale", 1.0).group("Emission").order(9).drag(0.05).clamp(0.0, 1000.0).desc("How fast time passes for the particles."),
    PropMeta::number("VelocityInheritance", 0.0).group("Emission").order(10).slider(0.0, 1.0).desc("How much of the part's speed the particles get."),
    PropMeta::enumeration("Shape", "ParticleEmitterShape", "Box").group("Emission").order(11).hidden().desc("Kept for scripts: particles always come from the face."),
    PropMeta::enumeration("ShapeStyle", "ParticleEmitterShapeStyle", "Volume").group("Emission").order(12).hidden().desc("Kept for scripts: particles always come from the face."),
    PropMeta::enumeration("ShapeInOut", "ParticleEmitterShapeInOut", "Outward").group("Emission").order(13).hidden().desc("Kept for scripts: particles always come from the face."),
    PropMeta::number("EmitCount", 0.0).group("Emission").order(14).hidden().runtime().desc("How many particles scripts have asked for with Emit (the renderer makes them)."),
    PropMeta::number("ClearCount", 0.0).group("Emission").order(15).hidden().runtime().desc("How often scripts have called Clear (the renderer removes the particles)."),
];

fn emit(c: &mut MethodCall) -> Result<ScriptValue, String> {
    let n = c.opt_number(0).unwrap_or(1.0).clamp(0.0, effects::MAX_PARTICLES as f64) as f32;
    let so_far = c.dm.num(c.this, "EmitCount", 0.0);
    c.dm.set_prop(c.this, "EmitCount", Value::Num(so_far + n))?;
    Ok(ScriptValue::Nil)
}

fn clear(c: &mut MethodCall) -> Result<ScriptValue, String> {
    let so_far = c.dm.num(c.this, "ClearCount", 0.0);
    c.dm.set_prop(c.this, "ClearCount", Value::Num(so_far + 1.0))?;
    Ok(ScriptValue::Nil)
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("Emit", emit).desc("Emits a number of particles at once (one by default), whether or not the emitter is enabled."),
    MethodDesc::new("Clear", clear).desc("Removes the particles."),
];

pub fn spread(dm: &DataModel, id: Id) -> bevy::math::Vec2 {
    match dm.get_prop(id, "SpreadAngle") {
        Some(Value::V2(v)) => v,
        _ => bevy::math::Vec2::ZERO,
    }
}

fn animate(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    let Some(frame) = dm.parent(id).and_then(|p| effects::parent_frame(dm, p)) else { return ctx.fx_clear(id) };
    let on_part = frame.size != bevy::math::Vec3::ZERO;
    let normal = face_axes(dm.str_ref(id, "EmissionDirection")).0;
    let spec = EmitterSpec {
        enabled: dm.flag(id, "Enabled"),
        rate: dm.num(id, "Rate", 20.0),
        lifetime: effects::read_range(dm, id, "Lifetime", (5.0, 10.0)),
        speed: effects::read_range(dm, id, "Speed", (5.0, 5.0)),
        heading: Heading::Local(normal),
        spread: spread(dm, id),
        acceleration: dm.vec3(id, "Acceleration"),
        drag: dm.num(id, "Drag", 0.0),
        size: effects::NumberSeq::read(dm, id, "Size", 1.0),
        color: effects::ColorSeq::read(dm, id, "Color"),
        transparency: effects::NumberSeq::read(dm, id, "Transparency", 0.0),
        rotation: effects::read_range(dm, id, "Rotation", (0.0, 0.0)),
        rot_speed: effects::read_range(dm, id, "RotSpeed", (0.0, 0.0)),
        z_offset: dm.num(id, "ZOffset", 0.0),
        facing: Facing::from_name(dm.str_ref(id, "Orientation")),
        locked_to_part: dm.flag(id, "LockedToPart"),
        time_scale: dm.num(id, "TimeScale", 1.0),
        inherit_velocity: dm.num(id, "VelocityInheritance", 0.0),
        area: if on_part { Area::Face(normal) } else { Area::Point },
        look: Look::new(dm.str_ref(id, "Texture"), Procedural::Soft, dm.num(id, "LightEmission", 0.0)),
    };
    let (emitted, cleared) = (dm.num(id, "EmitCount", 0.0), dm.num(id, "ClearCount", 0.0));
    let name = dm.name(id);
    effects::run_emitter(ctx, id, &name, &spec, frame, emitted, cleared);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("ParticleFx").describe("Draws the particles of the emitter (render::effects).").animate(animate);

pub static CLASS: ClassDesc = ClassDesc::new("ParticleEmitter", Some("Instance"))
    .describe("Puffs of particles.")
    .category("Effects")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "WorldObject", hint: "Select a part or an attachment first: particles come out of it." })
    .groups(&["Emission", "Appearance"])
    .components(&["Animated", "ParticleFx"])
    .props(PROPS)
    .methods(METHODS);
