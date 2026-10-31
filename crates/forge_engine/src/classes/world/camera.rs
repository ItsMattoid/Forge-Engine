//! Camera: a point of view. The workspace's `CurrentCamera` is what the game shows.
//!
//! `CameraView` turns the current camera into the Bevy camera while a game runs
//! (the editor has its own camera, so it creates none).

use bevy::prelude::*;

use crate::{
    compat,
    core::{ClassDesc, ComponentDesc, EnumDesc, GizmoShape, ParentRule, PropMeta, F_XFORM},
    dm::{DataModel, Id},
    render::{despawn_entity, entity_of, set_entity, Mode, RenderCtx},
};

static PROPS: &[PropMeta] = &[
    PropMeta::number("FieldOfView", 70.0).group("Camera").order(0).flags(F_XFORM).slider(1.0, 120.0).desc("Vertical field of view in degrees."),
    PropMeta::enumeration("CameraType", "CameraType", "Custom").group("Camera").order(1).desc("Informational: scripts that move the camera usually set this to Scriptable."),
    PropMeta::reference("CameraSubject", "Instance").group("Camera").order(2).runtime().desc("What the camera follows: a part (informational: a script moves the camera)."),
    PropMeta::cframe("Focus").group("Camera").order(3).runtime().hidden().desc("The point the camera looks at (informational)."),
    PropMeta::vector2("ViewportSize", 1280.0, 720.0).group("Camera").order(4).runtime().readonly().desc("The size in pixels of the picture the camera draws (the host updates it)."),
    PropMeta::number("NearPlaneZ", -0.1).group("Camera").order(5).readonly().hidden().desc("The distance of the near clipping plane (negative: in front of the camera)."),
    PropMeta::number("FieldOfViewMode", 0.0).hidden(),
    PropMeta::boolean("HeadLocked", true).group("Camera").order(6).hidden(),
    PropMeta::number("HeadScale", 1.0).group("Camera").order(7).hidden(),
];

pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::numbered("CameraType", &["Custom", "Scriptable", "Fixed", "Attach", "Watch", "Track", "Follow", "Orbital"], &[5, 6, 0, 1, 2, 3, 4, 7]),
    EnumDesc::new("CameraMode", &["Classic", "LockFirstPerson"]),
];

// ---- rendering ----------------------------------------------------------------------------------

fn render(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, _bits: u8) {
    if ctx.opts.mode != Mode::Game {
        return;
    }
    let current = dm.reference(dm.workspace, "CurrentCamera") == Some(id);
    if !current || !dm.in_workspace(id) {
        despawn_entity(ctx, dm, id);
        return;
    }
    let (pos, rot) = dm.world_cframe(id).unwrap_or((Vec3::ZERO, Quat::IDENTITY));
    let transform = Transform { translation: pos, rotation: rot, scale: Vec3::ONE };
    let fov = dm.num(id, "FieldOfView", 70.0).clamp(1.0, 120.0).to_radians();
    let projection = Projection::Perspective(PerspectiveProjection { fov, near: 0.1, far: 10_000.0, ..default() });
    match entity_of(dm, id) {
        Some(e) => compat::insert(ctx.commands, e, (transform, projection)),
        None => {
            let e = ctx.commands.spawn((Name::new("Camera"), Camera3d::default(), transform, projection, ctx.opts.msaa)).id();
            set_entity(dm, id, Some(e));
        }
    }
}

/// A little camera body with its viewing direction, so cameras can be seen and selected in the editor.
fn gizmo(dm: &DataModel, id: Id, out: &mut Vec<GizmoShape>) {
    if let Some((p, q)) = dm.world_cframe(id) {
        out.push(GizmoShape::Box { center: p, rotation: q, size: Vec3::new(2.0, 1.4, 2.0) });
        out.push(GizmoShape::Arrow { from: p, to: p + q * Vec3::NEG_Z * 6.0 });
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("CameraView")
    .describe("Renders the game from this camera when it is the workspace's CurrentCamera.")
    .requires(&["Transform"])
    .render(render)
    .gizmo(gizmo);

// ---- class --------------------------------------------------------------------------------------

pub static CLASS: ClassDesc = ClassDesc::new("Camera", Some("WorldObject"))
    .describe("A point of view.")
    .category("World")
    .creatable()
    .parent_rule(ParentRule::Service("Workspace"))
    .groups(&["Transform", "Camera"])
    .components(&["Transform", "CameraView"])
    .props(PROPS)
    .luau(include_str!("camera.luau"));
