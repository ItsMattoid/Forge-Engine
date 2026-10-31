//! Light: what every light has in common.
//!
//! `Light` declares the properties all lights share (`Color`, `Intensity`,
//! `CastShadows`, `Enabled`) and the shared half of the Bevy renderer: whether the
//! light is on, where it is, and when its Bevy entity has to be created or
//! refreshed. A concrete light (`PointLight`, `SpotLight`, ...) supplies the other
//! half - which Bevy light component to put on that entity - as an [`ApplyFn`].
//! Adding a new kind of light therefore means adding one file next to this one.
//!
//! A light's `CFrame` is *relative to its parent* when the parent has a transform
//! (a part), so a light inside a part follows it; inside the world or a folder it
//! is positioned in world space.

use bevy::prelude::*;

use crate::{
    compat,
    core::{ClassDesc, GizmoShape, PropMeta, F_VISUAL},
    dm::{DataModel, Id, D_V},
    render::{despawn_entity, entity_of, set_entity, srgb, RenderCtx},
};

/// Bevy's point and spot lights are measured in lumens: this many per unit of `Intensity`.
pub const LUMENS_PER_UNIT: f32 = 1_500_000.0;

/// `Range` for the lights that fade out with distance. Each such class lists it in its
/// own properties, so lights without a range (directional) simply leave it out.
pub const RANGE: PropMeta = PropMeta::number("Range", 16.0).group("Light").order(10).flags(F_VISUAL).slider(0.0, 500.0).desc("How far the light reaches.");

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Light").order(0).flags(F_VISUAL).desc("Turns the light on or off."),
    PropMeta::color3("Color", 1.0, 1.0, 1.0).group("Light").order(1).flags(F_VISUAL).desc("The colour of the light."),
    PropMeta::number("Intensity", 1.0)
        .label("Brightness")
        .group("Light")
        .order(2)
        .flags(F_VISUAL)
        .drag(0.05)
        .clamp(0.0, 1000.0)
        .aliases(&["Brightness"])
        .desc("How bright the light is."),
    PropMeta::boolean("CastShadows", false)
        .label("Shadows")
        .group("Shadows")
        .order(0)
        .flags(F_VISUAL)
        .aliases(&["Shadows"])
        .desc("Whether objects block the light and cast shadows (costs performance)."),
];

pub static CLASS: ClassDesc = ClassDesc::new("Light", Some("WorldObject"))
    .describe("Base class of all lights.")
    .relative_transform()
    .groups(&["Light", "Shadows", "Rendering"])
    .components(&["Transform"])
    .props(PROPS);

// ---- shared rendering ---------------------------------------------------------------------------

/// The values every Bevy light needs, read once by [`sync`].
pub struct Common {
    pub color: Color,
    /// The `Intensity` property; each light type converts it to its own Bevy unit.
    pub intensity: f32,
    pub shadows: bool,
}

/// The light-type-specific half of rendering: puts the Bevy light component on
/// `entity`. Called whenever the light is created or a visual property changed.
pub type ApplyFn = fn(&mut RenderCtx, &DataModel, Id, Entity, &Common);

/// Keeps a light's Bevy entity in step with the instance. Call this from a light
/// type's render hook with its [`ApplyFn`].
pub fn sync(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, bits: u8, apply: ApplyFn) {
    let on = dm.in_workspace(id) && dm.flag(id, "Enabled");
    let Some((pos, rot)) = dm.world_cframe(id).filter(|_| on) else {
        despawn_entity(ctx, dm, id);
        return;
    };
    // A light with a `Face` (spot lights) points out of that side instead of straight ahead.
    let rot = rot * face_rotation(dm.str_ref(id, "Face"));
    let transform = Transform { translation: pos, rotation: rot, scale: Vec3::ONE };
    let entity = entity_of(dm, id);

    // Only moved: no need to touch the light component.
    if let (Some(e), false) = (entity, bits & D_V != 0) {
        ctx.set_transform(e, transform);
        return;
    }

    let common = Common { color: srgb(dm.vec3(id, "Color")), intensity: dm.num(id, "Intensity", 1.0), shadows: dm.flag(id, "CastShadows") };
    let e = match entity {
        Some(e) => e,
        None => {
            let e = ctx.commands.spawn((Name::new(dm.name(id)), transform)).id();
            set_entity(dm, id, Some(e));
            e
        }
    };
    compat::insert(ctx.commands, e, transform);
    apply(ctx, dm, id, e, &common);
}

/// The turn that makes a light's forward direction (-Z, Roblox's Front) point out of a face (`Enum.NormalId`); a light
/// without a face points forward.
pub fn face_rotation(face: &str) -> Quat {
    let target = match face {
        "Back" => Vec3::Z,
        "Top" => Vec3::Y,
        "Bottom" => Vec3::NEG_Y,
        "Right" => Vec3::X,
        "Left" => Vec3::NEG_X,
        _ => return Quat::IDENTITY,
    };
    Quat::from_rotation_arc(Vec3::NEG_Z, target)
}

// ---- shared editor gizmos -----------------------------------------------------------------------

/// The direction the light shines in.
pub fn direction_gizmo(dm: &DataModel, id: Id, out: &mut Vec<GizmoShape>) {
    let Some((p, q)) = dm.world_cframe(id) else { return };
    out.push(GizmoShape::Arrow { from: p, to: p + q * Vec3::NEG_Z * 4.0 });
}

/// How far the light reaches, plus its direction.
pub fn range_gizmo(dm: &DataModel, id: Id, out: &mut Vec<GizmoShape>) {
    let Some((p, _)) = dm.world_cframe(id) else { return };
    out.push(GizmoShape::Sphere { center: p, radius: dm.num(id, "Range", 16.0) });
    direction_gizmo(dm, id, out);
}

/// The cone of a spot light: its outer edge (reaching `Range` along the cone's side) and
/// its fully lit inner edge, plus the direction. `OuterAngle`/`InnerAngle` are full angles
/// in degrees, clamped the same way the renderer clamps them.
pub fn cone_gizmo(dm: &DataModel, id: Id, out: &mut Vec<GizmoShape>) {
    let Some((p, q)) = dm.world_cframe(id) else { return };
    let range = dm.num(id, "Range", 16.0);
    let outer = (dm.num(id, "OuterAngle", 90.0) * 0.5).to_radians().clamp(0.01, std::f32::consts::FRAC_PI_2 - 0.01);
    let inner = (dm.num(id, "InnerAngle", 45.0) * 0.5).to_radians().clamp(0.0, outer);
    for half in [outer, inner] {
        out.push(GizmoShape::Cone { apex: p, rotation: q, length: range * half.cos(), radius: range * half.sin() });
    }
    direction_gizmo(dm, id, out);
}
