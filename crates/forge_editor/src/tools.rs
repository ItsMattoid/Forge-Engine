//! The transform tools in the viewport: what their handles are, what a drag does, and how they are drawn.
//!
//! * **Select**: no handles, but an object can be picked up (`Handle::Surface`): it is carried with its bounding box set down on
//!   whatever surface is under the mouse (the ground, another object, a wall), and slides on a plane over empty sky.
//! * **Move**: six arrows (both directions of each axis, starting a gap away from the centre and ending
//!   in a solid cone) and three plane handles that move along two axes at once. The planes sit on the
//!   side of the axes that faces the camera.
//! * **Scale**: six spheres standing off the faces of the bounding box. While dragging, Shift scales all
//!   three dimensions by the same factor, Ctrl scales both sides of the box about its centre, and Alt (when
//!   the handles are put on one part of a model) scales that part alone.
//! * **Rotate**: three rings, plus a trackball in the middle for free rotation.
//! * **Pivot**: the move handles, but dragging moves the object's pivot (its `PivotOffset`), not the object.
//!
//! With several objects selected the gizmo sits at the centre of their combined bounds and the tools act
//! on all of them. While a handle is being dragged the gizmo itself is hidden and only the measurement
//! is shown: a ruler marked every 5 units with the distance moved (move, pivot) or the new size (scale), or
//! a ring marked every few degrees, with the angle turned filled in (rotate).
//!
//! **Drags are measured from where they began.** Every frame recomputes the result from the objects' state
//! when the drag started and from the mouse, never from the previous frame's result, so nothing adds up
//! rounding error and a modifier key pressed or released half way through just changes the formula. The two
//! exceptions are things that cannot be known from the mouse alone: the angle of a ring (a direction only
//! gives an angle modulo a full turn, so the turns are counted, see [`gizmo::unwrap_angle`]) and the
//! trackball (a rotation cannot be recovered from where the mouse is, so it adds up small arcs).
//!
//! Lines are drawn with Bevy gizmos in their own group (`ToolGizmos`) whose depth bias puts them in front
//! of the scene without touching how the scene itself is depth tested. Filled shapes (cones, planes,
//! spheres, the sector of a rotation) and text can only be drawn in 2D, so they are an egui overlay clipped to
//! the viewport; that is above the scene by construction.

use std::collections::HashSet;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use forge_engine::{
    core::Value,
    diag::{timing, Metrics},
    dm::{DataModel, Id},
    physics::{ray_collider, ColliderInfo},
};

use crate::{
    gizmo::{self, BoxBounds, MoveHandle, TickKind, View, AXES},
    settings::Space,
    state::{Editor, Tool},
    viewport::{EditorCam, ViewportRect},
};

/// Gizmo group for the tools: always drawn in front of the scene.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ToolGizmos;

/// Called once at startup.
pub fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<ToolGizmos>();
    // -1 = in front of everything (the other gizmos, like the selection outline, keep the default).
    config.depth_bias = -1.0;
    config.line.width = 2.5;
}

// ---- colours ----------------------------------------------------------------------------------------

/// The same three muted colours for X, Y and Z in every tool: easy to tell apart, quiet against the scene.
const AXIS_RGB: [[u8; 3]; 3] = [[196, 100, 100], [118, 170, 118], [102, 138, 198]];
const HIGHLIGHT_RGB: [u8; 3] = [228, 210, 124];
/// The part of a model the scale handles are on when Alt is held.
const PART_RGB: [u8; 3] = [120, 220, 240];

fn bevy_color(rgb: [u8; 3], alpha: f32) -> Color {
    Color::srgba_u8(rgb[0], rgb[1], rgb[2], (alpha.clamp(0.0, 1.0) * 255.0) as u8)
}

fn egui_color(rgb: [u8; 3], alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], alpha)
}

// ---- state -------------------------------------------------------------------------------------------

/// A handle of the active tool.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Handle {
    Move(MoveHandle),
    /// A scale sphere: axis and side (+1 / -1).
    Scale(usize, f32),
    /// A rotation ring.
    Ring(usize),
    /// Free rotation inside the rings.
    Trackball,
    /// Not a handle: an object picked up where it was clicked, with the Select tool.
    Surface,
}

/// What the current tool shows for the selection.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub tool: Tool,
    pub origin: Vec3,
    pub dirs: [Vec3; 3],
    /// Length of the arrows and radius of the rotation rings (constant on screen).
    pub len: f32,
    /// The box the scale handles sit on.
    pub bounds: Option<BoxBounds>,
    /// That box is a single part's own (the selection outline already shows it).
    pub own_box: bool,
}

/// One object a drag moves: its pivot when the drag began.
#[derive(Clone, Copy, Debug)]
struct Start {
    id: Id,
    pos: Vec3,
    rot: Quat,
}

/// The world-aligned box around what a Select-tool drag carries.
#[derive(Clone, Copy, Debug)]
struct SnapBox {
    center: Vec3,
    half: Vec3,
}

/// A part of something scaled (a model, or several objects): where it was and how big, when the drag began.
#[derive(Clone, Copy, Debug)]
struct PartStart {
    id: Id,
    pos: Vec3,
    rot: Quat,
    /// `Vec3::ZERO` for something without a size (it only moves).
    size: Vec3,
    /// Its `PivotOffset`: scaled with it when everything is scaled by one factor.
    pivot_offset: Vec3,
}

/// A model inside what is scaled: its scale (`AppliedScale`) and pivot offset when the drag began. A uniform scale
/// of the contents is a scale of the model, so it is kept in step (see [`apply_scale`]).
#[derive(Clone, Copy, Debug)]
struct ModelStart {
    id: Id,
    scale: f32,
    pivot_offset: Vec3,
}

#[derive(Clone, Debug)]
pub struct Drag {
    handle: Handle,
    frame: Frame,
    /// The active object (the pivot tool acts on this one; for scale, the part the handles are on).
    active: Id,
    /// The rotation increment this drag snaps to (and marks its ring with); 0 = free.
    pub snap_step: f32,
    /// Where along the axis (arrows, spheres) the mouse took hold.
    t0: f32,
    /// Where on the plane (plane handles) or the ring / ball (rotation) it took hold, relative to the origin.
    grab: Vec3,
    /// The normal of the plane a picked-up object slides on when nothing is under the mouse to set it down on.
    plane: Vec3,
    /// A picked-up object: the other objects it can be set down on (their collision volumes when it was picked
    /// up, so each frame is a ray test and not a look at the whole scene), and the box that is set down.
    surfaces: Vec<ColliderInfo>,
    snap_box: Option<SnapBox>,
    starts: Vec<Start>,
    /// Scale: every part inside what is scaled, as it was.
    parts: Vec<PartStart>,
    /// Scale: every model inside what is scaled, as it was.
    models: Vec<ModelStart>,
    /// Ring: the raw angle last frame and the angle turned in total (radians), see [`gizmo::unwrap_angle`].
    ring: (f32, f32),
    /// Trackball: where the ball was last grabbed on its surface, and what the drag has turned it by.
    ball: (Vec3, Quat),
    /// What the drag has done so far, for the measurements.
    pub done: Done,
    /// How far a move drag (arrows, planes, a picked-up object) has carried the objects from `starts` right now.
    applied: Vec3,
    /// Whether Ctrl counts as held for the snapping, and when its real state differs from that, where the pointer
    /// was when the difference began (see [`Drag::ctrl_effective`]).
    ctrl_used: bool,
    ctrl_changed_at: Option<Option<Vec2>>,
}

impl Drag {
    /// What is being dragged.
    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// The drag starts with Ctrl as it is held now (so holding it from the start is not a change).
    pub fn begin_with_ctrl(&mut self, ctrl: bool) {
        self.ctrl_used = ctrl;
    }

    /// Whether Ctrl ("ignore the snapping") counts as held this frame. Pressing or letting go of Ctrl mid-drag only
    /// changes the snapping from the next movement of the mouse on: until then the objects stay where the old
    /// meaning put them. Without that, taking hold of Ctrl for a shortcut such as Ctrl+R would move what is being
    /// carried by up to half a snapping step before the second key was even pressed.
    pub fn ctrl_effective(&mut self, ctrl: bool, pointer: Option<Vec2>) -> bool {
        if ctrl == self.ctrl_used {
            self.ctrl_changed_at = None;
            return ctrl;
        }
        match self.ctrl_changed_at {
            None => self.ctrl_changed_at = Some(pointer),
            Some(at) if at != pointer => {
                self.ctrl_used = ctrl;
                self.ctrl_changed_at = None;
            }
            Some(_) => {}
        }
        self.ctrl_used
    }
}

/// The measurements of a drag in progress.
#[derive(Clone, Copy, Debug, Default)]
pub struct Done {
    /// Distance moved along each of the gizmo's axes (move and pivot tools).
    pub moved: [f32; 3],
    /// Angle turned, degrees (rotate tool). For a ring it is the whole angle, past 180 and 360 degrees too.
    pub angle: f32,
    /// How far the grabbed face has moved from where it was (scale tool; negative when it moved in).
    pub pull: f32,
    /// The box's size along the grabbed axis now, and the factor that is of what it was (scale tool).
    pub extent: f32,
    pub factor: f32,
    /// Which modifiers shaped the scale: all three sizes by one factor / about the centre.
    pub proportional: bool,
    pub from_center: bool,
}

/// An object under the mouse that has been pressed, and that becomes a drag if the mouse moves while the
/// button stays down (a click that does not move only selects).
#[derive(Clone, Copy, Debug)]
pub struct Press {
    /// Where the pointer was (screen points).
    pub pointer: Vec2,
    pub id: Id,
    /// The point of the object under the pointer, and the ray that found it.
    pub point: Vec3,
    pub ray: (Vec3, Vec3),
    /// The object was one of several selected: the selection is only narrowed to it if this turns out to
    /// be a click, so that dragging one of a multiple selection still moves them all.
    pub collapse: bool,
}

/// What the pointer is over, and what is being dragged.
#[derive(Resource, Default)]
pub struct GizmoState {
    pub drag: Option<Drag>,
    /// The handle under the mouse.
    pub hover: Option<Handle>,
    /// Scale tool with Alt held: the part of the selected model whose box the handles are on.
    pub part: Option<Id>,
    /// An object pressed with the Select tool that may become a drag.
    pub press: Option<Press>,
}

impl GizmoState {
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

// ---- the frame of the active tool -------------------------------------------------------------------

/// The world-aligned box (min, max) around the collision volumes of these objects and everything inside them.
fn aabb_of(dm: &DataModel, ids: &[Id]) -> Option<(Vec3, Vec3)> {
    let (center, half) = gizmo::union_box(ids.iter().flat_map(|id| dm.subtree(*id)).filter_map(|n| dm.collider(n)).map(|c| c.aabb()))?;
    Some((center - half, center + half))
}

/// The box the scale handles go on, and whether it is a single part's own box: that part's box if one part
/// with a size is selected, otherwise the world-aligned box around everything selected (a model's contents,
/// or several objects together).
pub fn scale_bounds(dm: &DataModel, targets: &[Id]) -> Option<(BoxBounds, bool)> {
    if let [only] = targets {
        if dm.get(*only)?.class.prop("Size").is_some() {
            // The part's centre, not its pivot (a pivot offset must not move the box).
            let (center, rot) = dm.world_cframe(*only)?;
            let half = (dm.size(*only) * 0.5).max(Vec3::splat(0.001));
            return Some((BoxBounds { center, half, rot }, true));
        }
    }
    let (min, max) = aabb_of(dm, targets)?;
    Some((BoxBounds { center: (min + max) * 0.5, half: ((max - min) * 0.5).max(Vec3::splat(0.001)), rot: Quat::IDENTITY }, false))
}

/// Where the tool's handles are for the selection (`None` when the tool has none for it).
/// `targets` are the objects the tool acts on; `active` is the one it is attached to when there is one object.
pub fn frame_for(dm: &DataModel, tool: Tool, space: Space, active: Id, targets: &[Id], camera: Vec3) -> Option<Frame> {
    let (pivot, pivot_rot) = dm.get_pivot(active)?;
    // The pivot tool edits the active object's pivot only.
    let targets: Vec<Id> = if tool == Tool::Pivot || targets.is_empty() { vec![active] } else { targets.to_vec() };
    let (origin, rot, bounds, own_box) = match tool {
        Tool::Select => return None,
        Tool::Pivot => (pivot, pivot_rot, None, false),
        Tool::Move | Tool::Rotate => {
            // One object: its pivot. Several: the middle of everything they cover.
            let origin = if targets.len() > 1 { aabb_of(dm, &targets).map_or(pivot, |(a, b)| (a + b) * 0.5) } else { pivot };
            (origin, pivot_rot, None, false)
        }
        Tool::Scale => {
            let (b, own) = scale_bounds(dm, &targets)?;
            (b.center, b.rot, Some(b), own)
        }
    };
    Some(Frame { tool, origin, dirs: gizmo::axis_dirs(tool, space, rot), len: gizmo::handle_len(camera, origin), bounds, own_box })
}

/// The frame of an object picked up with the Select tool: a point, and the world's axes.
pub fn surface_frame(point: Vec3) -> Frame {
    Frame { tool: Tool::Select, origin: point, dirs: AXES, len: 1.0, bounds: None, own_box: false }
}

/// The handle under the mouse.
pub fn hit(tool: Tool, frame: &Frame, view: &View, pointer: Vec2) -> Option<Handle> {
    let ray = view.ray(pointer)?;
    let camera = view.pos;
    match tool {
        Tool::Select => None,
        Tool::Move | Tool::Pivot => gizmo::hit_move(view, pointer, frame.origin, &frame.dirs).map(Handle::Move),
        Tool::Scale => frame.bounds.and_then(|b| gizmo::hit_scale(camera, ray, &b)).map(|(a, s)| Handle::Scale(a, s)),
        Tool::Rotate => {
            // An axis ring wins over the trackball: the ball only starts where no ring is under the mouse.
            gizmo::hit_ring(camera, ray, frame.origin, &frame.dirs).map(Handle::Ring).or_else(|| gizmo::inside_trackball(ray, frame.origin, frame.len * 0.9).then_some(Handle::Trackball))
        }
    }
}

// ---- starting and running a drag ------------------------------------------------------------------------

fn part_start(dm: &DataModel, id: Id) -> Option<PartStart> {
    let (pos, rot) = dm.world_cframe(id)?;
    let size = if dm.get(id).is_some_and(|i| i.class.prop("Size").is_some()) { dm.size(id) } else { Vec3::ZERO };
    Some(PartStart { id, pos, rot, size, pivot_offset: dm.vec3(id, "PivotOffset") })
}

/// Takes hold of `handle`. `targets` are the objects the tool carries along (the selected objects, without
/// any that are inside another selected object). For [`Handle::Surface`] `frame.origin` is the point that
/// was clicked on.
pub fn begin_drag(dm: &DataModel, handle: Handle, frame: Frame, ray: (Vec3, Vec3), targets: &[Id], active: Id) -> Option<Drag> {
    let starts: Vec<Start> = targets.iter().filter_map(|id| dm.get_pivot(*id).map(|(pos, rot)| Start { id: *id, pos, rot })).collect();
    let mut drag = Drag {
        handle,
        frame,
        active,
        snap_step: 15.0,
        t0: 0.0,
        grab: Vec3::ZERO,
        plane: Vec3::Y,
        surfaces: Vec::new(),
        snap_box: None,
        starts,
        parts: Vec::new(),
        models: Vec::new(),
        ring: (0.0, 0.0),
        ball: (Vec3::ZERO, Quat::IDENTITY),
        done: Done { factor: 1.0, ..Done::default() },
        applied: Vec3::ZERO,
        ctrl_used: false,
        ctrl_changed_at: None,
    };
    match handle {
        Handle::Move(MoveHandle::Arrow(axis, _)) => {
            drag.t0 = gizmo::ray_line(ray.0, ray.1, frame.origin, frame.dirs[axis])?.1;
        }
        Handle::Move(MoveHandle::Plane(normal)) => {
            drag.grab = gizmo::ray_handle_plane(ray, frame.origin, frame.dirs[normal])? - frame.origin;
        }
        Handle::Ring(axis) => {
            drag.grab = gizmo::ray_plane(ray, frame.origin, frame.dirs[axis])? - frame.origin;
            // The angle is counted from here: zero now.
            drag.ring = (gizmo::signed_angle(frame.dirs[axis], drag.grab, drag.grab), 0.0);
        }
        Handle::Trackball => {
            drag.ball = (gizmo::trackball_point(ray, frame.origin, frame.len * 0.9), Quat::IDENTITY);
        }
        Handle::Surface => {
            drag.plane = gizmo::surface_drag_normal(ray.1);
            // The click must land on the plane it will slide on (it does, by construction).
            gizmo::ray_handle_plane(ray, frame.origin, drag.plane)?;
            // What it can be set down on: everything that is not being carried.
            let carried: HashSet<Id> = targets.iter().flat_map(|t| dm.subtree(*t)).collect();
            drag.surfaces = dm.colliders().into_iter().filter(|id| !carried.contains(id) && dm.in_workspace(*id)).filter_map(|id| dm.collider(id)).collect();
            if let Some((min, max)) = aabb_of(dm, targets) {
                drag.snap_box = Some(SnapBox { center: (min + max) * 0.5, half: (max - min) * 0.5 });
            }
        }
        Handle::Scale(axis, sign) => {
            let bounds = frame.bounds?;
            // Measured along the same line the drag follows (through the face), so grabbing the sphere that stands off the face is not a jump.
            drag.t0 = gizmo::ray_line(ray.0, ray.1, bounds.face(axis, sign), bounds.outward(axis, sign))?.1;
            drag.parts = if frame.own_box {
                // One part: it is scaled by itself, whatever is inside it.
                part_start(dm, active).into_iter().collect()
            } else {
                // A model or several objects: every part inside is stretched along the axis, so remember each one's start.
                targets.iter().flat_map(|t| dm.subtree(*t)).filter(|n| dm.has_component(*n, "Transform") && !dm.get(*n).is_some_and(|i| i.class.relative_transform)).filter_map(|n| part_start(dm, n)).collect()
            };
            // The models inside (a model can be selected, or be what a selected part is in): their scale follows.
            if !frame.own_box {
                drag.models = targets.iter().flat_map(|t| dm.subtree(*t)).filter(|n| dm.get(*n).is_some_and(|i| i.class.prop("AppliedScale").is_some())).map(|id| ModelStart { id, scale: dm.applied_scale(id), pivot_offset: dm.vec3(id, "PivotOffset") }).collect();
            }
        }
    }
    Some(drag)
}

/// What a drag needs besides the mouse: the snapping steps, and the modifier keys as they are right now.
#[derive(Clone, Copy, Debug, Default)]
pub struct Snap {
    pub move_step: f32,
    pub turn_step: f32,
    /// Shift: scale all three dimensions by the same factor.
    pub proportional: bool,
    /// Ctrl: scale both sides of the box equally about its centre.
    pub from_center: bool,
}

impl Snap {
    /// Snapping steps, no modifier keys.
    #[cfg(test)]
    pub fn steps(move_step: f32, turn_step: f32) -> Snap {
        Snap { move_step, turn_step, ..Snap::default() }
    }
}

/// Applies the mouse ray to a drag: moves, turns or resizes the objects and records the measurements.
/// Returns whether anything changed.
pub fn update_drag(dm: &mut DataModel, drag: &mut Drag, ray: (Vec3, Vec3), snap: &Snap) -> bool {
    let f = drag.frame;
    match drag.handle {
        Handle::Move(MoveHandle::Arrow(axis, _)) => {
            let Some((_, t)) = gizmo::ray_line(ray.0, ray.1, f.origin, f.dirs[axis]) else { return false };
            let amount = gizmo::snap(t - drag.t0, snap.move_step);
            drag.done.moved = [0.0; 3];
            drag.done.moved[axis] = amount;
            displace(dm, drag, f.dirs[axis] * amount)
        }
        Handle::Move(MoveHandle::Plane(normal)) => {
            // (Seen edge-on the plane cannot be followed: the object stays where it is until the view allows it.)
            let Some(p) = gizmo::ray_handle_plane(ray, f.origin, f.dirs[normal]) else { return false };
            let rel = p - f.origin - drag.grab;
            let (a, b) = gizmo::plane_axes(normal);
            let (da, db) = (gizmo::snap(rel.dot(f.dirs[a]), snap.move_step), gizmo::snap(rel.dot(f.dirs[b]), snap.move_step));
            drag.done.moved = [0.0; 3];
            drag.done.moved[a] = da;
            drag.done.moved[b] = db;
            displace(dm, drag, f.dirs[a] * da + f.dirs[b] * db)
        }
        Handle::Ring(axis) => {
            let Some(p) = gizmo::ray_plane(ray, f.origin, f.dirs[axis]) else { return false };
            let v = p - f.origin;
            if v.length_squared() < 1e-8 {
                return false;
            }
            // The angle between two directions is only known modulo a full turn: count the turns.
            let raw = gizmo::signed_angle(f.dirs[axis], drag.grab, v);
            drag.ring = (raw, gizmo::unwrap_angle(drag.ring.1, drag.ring.0, raw));
            let degrees = gizmo::snap(drag.ring.1.to_degrees(), snap.turn_step);
            drag.done.angle = degrees;
            turn_all(dm, drag, Quat::from_axis_angle(f.dirs[axis], degrees.to_radians()))
        }
        Handle::Trackball => {
            // A rotation cannot be read off the mouse's position (the arc from where it began is never more than
            // half a turn, and both ends are on the ball's visible half): add up the small arcs from frame to frame.
            let now = gizmo::trackball_point(ray, f.origin, f.len * 0.9);
            let step = gizmo::trackball_rotation(drag.ball.0, now);
            drag.ball = (now, (step * drag.ball.1).normalize());
            drag.done.angle = drag.ball.1.angle_between(Quat::IDENTITY).to_degrees();
            turn_all(dm, drag, drag.ball.1)
        }
        Handle::Scale(axis, sign) => scale(dm, drag, ray, snap, axis, sign),
        Handle::Surface => {
            // Over another object, the carried box is set down on its surface, centred on the mouse (see `set_down`).
            if let (Some(b), Some((point, normal))) = (drag.snap_box, nearest_surface(&drag.surfaces, ray)) {
                let delta = set_down(&b, point, normal, snap.move_step);
                return move_all(dm, drag, delta);
            }
            // Over nothing (the sky), an object slides on a plane through where it was grabbed, keeping the
            // offset between the cursor and the object it had at the start.
            let Some(p) = gizmo::ray_handle_plane(ray, f.origin, drag.plane) else { return false };
            let delta = (p - f.origin).map(|c| gizmo::snap(c, snap.move_step));
            move_all(dm, drag, delta)
        }
    }
}

/// The nearest surface a ray meets among `surfaces`: the point and the normal that faces the ray.
fn nearest_surface(surfaces: &[ColliderInfo], ray: (Vec3, Vec3)) -> Option<(Vec3, Vec3)> {
    surfaces.iter().filter_map(|info| ray_collider(info, ray.0, ray.1)).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(t, normal)| (ray.0 + ray.1 * t, normal))
}

/// How far the carried box has to move to be set down on the surface at `point` with outward `normal`: centred
/// on the point the mouse is at, with its side towards the surface lying on it (touching, neither sunk in nor
/// hovering). With a surface lined up with an axis its place along the surface is rounded to the move
/// increment (in world coordinates, so objects land on the same grid); across it, never, or the box would
/// not touch.
fn set_down(b: &SnapBox, point: Vec3, normal: Vec3, step: f32) -> Vec3 {
    // How far the box reaches towards the surface from its centre, whichever way it is turned.
    let reach = normal.abs().dot(b.half);
    let mut center = point + normal * reach;
    if let Some(k) = (0..3).find(|k| normal[*k].abs() > 0.999) {
        for j in (0..3).filter(|j| *j != k) {
            center[j] = gizmo::snap(center[j], step);
        }
    }
    center - b.center
}

/// What a move drag does: the move tool moves every dragged object by `delta`; the pivot tool moves
/// the active object's pivot there and leaves the object where it is.
fn displace(dm: &mut DataModel, drag: &mut Drag, delta: Vec3) -> bool {
    if drag.frame.tool == Tool::Pivot {
        let target = drag.frame.origin + delta;
        let differs = dm.get_pivot(drag.active).is_some_and(|(p, _)| p.distance_squared(target) > 1e-10);
        return differs && dm.set_pivot_point(drag.active, target).is_ok();
    }
    move_all(dm, drag, delta)
}

/// Moves every dragged object to where it started plus `delta`.
fn move_all(dm: &mut DataModel, drag: &mut Drag, delta: Vec3) -> bool {
    drag.applied = delta;
    let mut changed = false;
    for s in &drag.starts {
        let target = s.pos + delta;
        if dm.get_pivot(s.id).is_some_and(|(p, _)| p.distance_squared(target) > 1e-10) {
            dm.pivot_to(s.id, (target, s.rot));
            changed = true;
        }
    }
    changed
}

/// Turns every dragged object by `turn` around the gizmo's origin (an object away from the origin swings round it).
fn turn_all(dm: &mut DataModel, drag: &Drag, turn: Quat) -> bool {
    let mut changed = false;
    for s in &drag.starts {
        let pos = drag.frame.origin + turn * (s.pos - drag.frame.origin);
        let rot = (turn * s.rot).normalize();
        let differs = dm.get_pivot(s.id).is_some_and(|(p, q)| p.distance_squared(pos) > 1e-10 || q.angle_between(rot) > 1e-5);
        if differs {
            dm.pivot_to(s.id, (pos, rot));
            changed = true;
        }
    }
    changed
}

/// Turns `targets` a quarter turn around the X (`axis` 0) or Y (`axis` 1) axis, as ONE group: every object swings
/// round the same point, so how they sit relative to each other is kept. The point and the axes are the Rotate
/// tool's own (see [`frame_for`]): the pivot of a single object, or the middle of everything; the world's axes, or in
/// Local space the active object's. `active` leads (it gives the Local axes); failing that the first target.
///
/// This is the one implementation of the Rotate-90 and Flip shortcuts. `drag` is the drag in progress, if there is
/// one: a drag measures every frame from where the objects were when it began, so turning them behind its back would
/// be undone by its next frame. Instead the turn is taken into the drag (see [`Drag::take_in_turn`]) and it goes on
/// from the turned objects, under the same mouse, pivot, space and snapping. A Scale drag cannot take a turn in (it
/// stretches along the box's axes, which a turn would change under it): nothing happens then. Returns whether
/// anything was turned.
pub fn quarter_turn(dm: &mut DataModel, mut drag: Option<&mut Drag>, space: Space, targets: &[Id], active: Option<Id>, axis: usize) -> bool {
    if drag.as_ref().is_some_and(|d| matches!(d.handle, Handle::Scale(..))) {
        return false;
    }
    let targets: Vec<Id> = targets.iter().copied().filter(|id| dm.get_pivot(*id).is_some()).collect();
    let Some(active) = active.filter(|a| targets.contains(a)).or_else(|| targets.first().copied()) else { return false };
    let Some(frame) = frame_for(dm, Tool::Rotate, space, active, &targets, Vec3::ZERO) else { return false };
    let turn = Quat::from_axis_angle(frame.dirs[axis], std::f32::consts::FRAC_PI_2);
    // Where everything is before anything moves (turning one object must not change what the next starts from).
    let starts: Vec<(Id, Vec3, Quat)> = targets.iter().filter_map(|id| dm.get_pivot(*id).map(|(p, q)| (*id, p, q))).collect();
    for (id, pos, rot) in &starts {
        dm.pivot_to(*id, (frame.origin + turn * (*pos - frame.origin), (turn * *rot).normalize()));
    }
    if let Some(drag) = drag.as_deref_mut() {
        drag.take_in_turn(dm);
    }
    true
}

impl Drag {
    /// After the objects were turned by something other than the drag: makes the drag start from where they are now.
    /// Each kind of drag measures from `starts` by its own formula, so the starts are chosen such that the formula,
    /// with the mouse where it is, gives exactly the objects as they are now - and from there on the mouse carries
    /// them as before.
    fn take_in_turn(&mut self, dm: &DataModel) {
        let now = |id: Id| dm.get_pivot(id);
        match self.handle {
            // The objects are `starts + applied` (and keep the start's orientation): shift the starts back by what
            // was applied. A picked-up object is also set down by the size of the box around it, which a turn changes.
            Handle::Move(_) | Handle::Surface if self.frame.tool != Tool::Pivot => {
                for s in &mut self.starts {
                    if let Some((pos, rot)) = now(s.id) {
                        s.pos = pos - self.applied;
                        s.rot = rot;
                    }
                }
                if self.handle == Handle::Surface {
                    let ids: Vec<Id> = self.starts.iter().map(|s| s.id).collect();
                    if let Some((min, max)) = aabb_of(dm, &ids) {
                        self.snap_box = Some(SnapBox { center: (min + max) * 0.5 - self.applied, half: (max - min) * 0.5 });
                    }
                }
            }
            // The objects are `turned(starts)` about the gizmo's origin: undo the drag's own turn on the new state to
            // get starts that the drag's turn lands on it again.
            Handle::Ring(_) | Handle::Trackball if self.frame.tool == Tool::Rotate => {
                let own = match self.handle {
                    Handle::Ring(axis) => Quat::from_axis_angle(self.frame.dirs[axis.min(2)], self.done.angle.to_radians()),
                    _ => self.ball.1,
                };
                let back = own.inverse();
                let origin = self.frame.origin;
                for s in &mut self.starts {
                    if let Some((pos, rot)) = now(s.id) {
                        s.pos = origin + back * (pos - origin);
                        s.rot = (back * rot).normalize();
                    }
                }
            }
            // The Pivot tool's drag only sets the pivot point (the objects are not carried by it).
            _ => {}
        }
    }
}

/// The smallest a box is made along the axis being dragged.
const MIN_EXTENT: f32 = 0.05;
/// The smallest a part is made along any axis by scaling.
const MIN_SIZE: f32 = 0.001;

/// Dragging a scale sphere. The grabbed face follows the mouse; what the other side does depends on the modifiers:
/// * plain: the opposite face stays where it is;
/// * Ctrl: the centre stays where it is, so the opposite face moves the other way by the same amount;
/// * Shift: the other two dimensions grow by the same factor (about the opposite face's centre, or about the
///   centre with Ctrl), so the proportions are kept.
/// The grabbed face is under the mouse in every case, so pressing or releasing a key mid-drag moves the other
/// side of the box and the other dimensions but never the handle being held.
fn scale(dm: &mut DataModel, drag: &mut Drag, ray: (Vec3, Vec3), snap: &Snap, axis: usize, sign: f32) -> bool {
    let Some(bounds) = drag.frame.bounds else { return false };
    let outward = bounds.outward(axis, sign);
    let Some((_, t)) = gizmo::ray_line(ray.0, ray.1, bounds.face(axis, sign), outward) else { return false };
    let extent0 = bounds.half[axis] * 2.0;
    // With the centre fixed a face has to move by half the growth of the box.
    let reach = if snap.from_center { 2.0 } else { 1.0 };
    let wanted = extent0 + reach * gizmo::snap(t - drag.t0, snap.move_step);
    let extent = wanted.max(MIN_EXTENT);
    let factor = extent / extent0.max(1e-4);
    drag.done = Done { pull: (extent - extent0) / reach, extent, factor, proportional: snap.proportional, from_center: snap.from_center, ..drag.done };

    let scale = if snap.proportional {
        Vec3::splat(factor)
    } else {
        let mut s = Vec3::ONE;
        s[axis] = factor;
        s
    };
    let anchor = if snap.from_center { bounds.center } else { bounds.face(axis, -sign) };
    apply_scale(dm, &drag.parts, &drag.models, &bounds, anchor, scale, snap.proportional)
}

/// Scales every part about `anchor`: positions by `scale` along the box's three axes, sizes by the factor of the
/// axis each of their own axes lines up with. With `uniform` (all three factors equal) every size is scaled,
/// whatever the part's orientation; otherwise a part that is turned between the box's axes cannot be stretched
/// without shearing, so only its position follows.
///
/// A uniform scale is also what `Model.Scale` means, so it takes the models inside along: their `AppliedScale` and
/// everyone's `PivotOffset` are the start values times the factor (and are back at the start values while the scale
/// is not uniform), which is what changing `Scale` in the Properties panel would have done.
fn apply_scale(dm: &mut DataModel, parts: &[PartStart], models: &[ModelStart], bounds: &BoxBounds, anchor: Vec3, scale: Vec3, uniform: bool) -> bool {
    let axes = AXES.map(|a| bounds.rot * a);
    let mut changed = false;
    let factor = if uniform { scale.x } else { 1.0 };
    for m in models {
        let wanted = (m.scale * factor).clamp(forge_engine::classes::world::model::MIN_SCALE, forge_engine::classes::world::model::MAX_SCALE);
        if dm.applied_scale(m.id) != wanted && dm.set_prop(m.id, "AppliedScale", Value::Num(wanted)).is_ok() {
            changed = true;
        }
        if dm.vec3(m.id, "PivotOffset") != m.pivot_offset * factor && dm.set_prop(m.id, "PivotOffset", Value::V3(m.pivot_offset * factor)).is_ok() {
            changed = true;
        }
    }
    for p in parts {
        let rel = p.pos - anchor;
        let pos = anchor + (0..3).map(|k| axes[k] * (rel.dot(axes[k]) * scale[k])).sum::<Vec3>();
        let mut size = p.size;
        if size != Vec3::ZERO {
            for j in 0..3 {
                let own = p.rot * AXES[j];
                let factor = if uniform { Some(scale.x) } else { (0..3).find(|k| own.dot(axes[*k]).abs() > 0.98).map(|k| scale[k]) };
                if let Some(factor) = factor {
                    size[j] = (p.size[j] * factor).max(MIN_SIZE);
                }
            }
        }
        if size != Vec3::ZERO && dm.size(p.id) != size {
            let _ = dm.set_prop(p.id, "Size", Value::V3(size));
            changed = true;
        }
        if dm.get(p.id).is_some_and(|i| i.class.prop("PivotOffset").is_some()) && dm.vec3(p.id, "PivotOffset") != p.pivot_offset * factor {
            let _ = dm.set_prop(p.id, "PivotOffset", Value::V3(p.pivot_offset * factor));
            changed = true;
        }
        if dm.world_cframe(p.id).is_some_and(|(q, _)| q.distance_squared(pos) > 1e-10) {
            dm.set_cframe(p.id, pos, p.rot);
            changed = true;
        }
    }
    changed
}

// ---- drawing: lines (Bevy gizmos, in front of the scene) --------------------------------------------------------

fn axis_color(i: usize, active: bool, alpha: f32) -> Color {
    bevy_color(if active { HIGHLIGHT_RGB } else { AXIS_RGB[i] }, alpha)
}

/// How a move arrow is built: the shaft runs from the gap to the base of the cone, the cone to the tip.
fn arrow_parts(frame: &Frame, axis: usize, sign: f32) -> (Vec3, Vec3, Vec3) {
    let dir = frame.dirs[axis] * sign;
    let start = frame.origin + dir * (frame.len * gizmo::ARROW_GAP);
    let base = frame.origin + dir * (frame.len * (1.0 - gizmo::CONE_LEN));
    let tip = frame.origin + dir * frame.len;
    (start, base, tip)
}

/// Draws the tool's handles, or while dragging only the measurement guides (the gizmo steps out of the way).
pub fn draw_lines(gizmos: &mut Gizmos<ToolGizmos>, frame: &Frame, camera: &View, state: &GizmoState) {
    if let Some(drag) = &state.drag {
        draw_measurement(gizmos, drag, camera);
        return;
    }
    let lit = |h: Handle| state.hover == Some(h);
    let origin = frame.origin;
    match frame.tool {
        Tool::Select => {}
        Tool::Move | Tool::Pivot => {
            let signs = gizmo::facing_signs(camera.pos, origin, &frame.dirs);
            for i in 0..3 {
                for sign in [1.0f32, -1.0] {
                    // The two directions of an axis are one colour; the one pointing away from you is a little fainter.
                    let alpha = if sign == signs[i] { 1.0 } else { 0.6 };
                    let (start, base, _) = arrow_parts(frame, i, sign);
                    gizmos.line(start, base, axis_color(i, lit(Handle::Move(MoveHandle::Arrow(i, sign))), alpha));
                }
                let c = gizmo::plane_corners(origin, &frame.dirs, &signs, i, frame.len);
                gizmos.linestrip([c[0], c[1], c[2], c[3], c[0]], axis_color(i, lit(Handle::Move(MoveHandle::Plane(i))), 0.9));
            }
        }
        Tool::Scale => {
            if let Some(b) = frame.bounds {
                if frame.own_box {
                    // A part picked out of a model with Alt: its own box, which the selection outline does not single out.
                    if state.part.is_some() {
                        gizmos.cube(Transform { translation: b.center, rotation: b.rot, scale: b.half * 2.0 }, bevy_color(PART_RGB, 0.9));
                    }
                } else {
                    // For a model or several objects the box they sit on is shown, since no object outline shows it.
                    gizmos.cube(Transform { translation: b.center, rotation: b.rot, scale: b.half * 2.0 }, Color::srgba(1.0, 1.0, 1.0, 0.2));
                }
            }
        }
        Tool::Rotate => {
            for i in 0..3 {
                gizmos.circle(Isometry3d::new(origin, Quat::from_rotation_arc(Vec3::Z, frame.dirs[i])), frame.len * 0.9, axis_color(i, lit(Handle::Ring(i)), 0.95));
            }
        }
    }
}

/// The rulers, scale guide and rotation marks of a drag in progress.
fn draw_measurement(gizmos: &mut Gizmos<ToolGizmos>, drag: &Drag, camera: &View) {
    let f = &drag.frame;
    let to_camera = (camera.pos - f.origin).normalize_or_zero();
    match drag.handle {
        Handle::Move(MoveHandle::Arrow(axis, _)) => ruler(gizmos, &Ruler::along(f, axis), drag.done.moved[axis], to_camera),
        Handle::Move(MoveHandle::Plane(normal)) => {
            let (a, b) = gizmo::plane_axes(normal);
            ruler(gizmos, &Ruler::along(f, a), drag.done.moved[a], to_camera);
            ruler(gizmos, &Ruler::along(f, b), drag.done.moved[b], to_camera);
        }
        Handle::Ring(axis) => {
            let radius = f.len * 0.9;
            let ring_axis = f.dirs[axis];
            let zero = drag.grab.normalize_or_zero();
            let color = |alpha: f32| bevy_color(AXIS_RGB[axis], alpha);
            // The ring itself, so the marks have something to stand on.
            gizmos.circle(Isometry3d::new(f.origin, Quat::from_rotation_arc(Vec3::Z, ring_axis)), radius, color(0.95));
            // Marks outside the ring, each one starting on it: 0 is where the drag took hold, and the longest
            // are the quarter turns, then the eighths.
            let ring_on_screen = radius * camera.pixels_per_unit(f.origin);
            for (degrees, kind) in gizmo::tick_marks(drag.snap_step, ring_on_screen) {
                let dir = Quat::from_axis_angle(ring_axis, degrees.to_radians()) * zero;
                let alpha = match kind {
                    TickKind::Quarter => 1.0,
                    TickKind::Eighth => 0.85,
                    TickKind::Regular => 0.65,
                };
                gizmos.line(f.origin + dir * radius, f.origin + dir * (radius * (1.0 + kind.length())), color(alpha));
            }
            // The two edges of the angle turned (the fill between them is drawn with the overlay).
            let now = Quat::from_axis_angle(ring_axis, drag.done.angle.to_radians()) * zero;
            gizmos.line(f.origin, f.origin + zero * radius, color(0.55));
            gizmos.line(f.origin, f.origin + now * radius, color(0.95));
        }
        Handle::Trackball => {
            gizmos.circle(Isometry3d::new(f.origin, camera.rot), f.len * 0.9, Color::srgba(1.0, 1.0, 1.0, 0.6));
        }
        Handle::Scale(axis, sign) => {
            let Some(b) = f.bounds else { return };
            // The same ruler as the move tool, along the axis being scaled, from where the grabbed face was to where it is.
            let outward = b.outward(axis, sign);
            let r = Ruler { origin: b.face(axis, sign), dir: outward, rgb: AXIS_RGB[axis], size: f.len };
            ruler(gizmos, &r, drag.done.pull, to_camera);
            if drag.done.from_center {
                // The opposite face moves too: mark where it is now.
                cross_tick(gizmos, b.face(axis, -sign) - outward * drag.done.pull, outward, to_camera, f.len * 0.06 * 2.4, bevy_color(HIGHLIGHT_RGB, 1.0));
            }
        }
        // Picked up with the Select tool: the object itself is the feedback.
        Handle::Surface => {}
    }
}

/// A measuring line: along `dir` from `origin`, marked every 5 units, with `size` the size of the gizmo it belongs to.
struct Ruler {
    origin: Vec3,
    dir: Vec3,
    rgb: [u8; 3],
    size: f32,
}

impl Ruler {
    /// Along one of the gizmo's axes, from its origin.
    fn along(f: &Frame, axis: usize) -> Ruler {
        Ruler { origin: f.origin, dir: f.dirs[axis], rgb: AXIS_RGB[axis], size: f.len }
    }
}

/// A short line across a ruler, on the side towards the viewer.
fn cross_tick(gizmos: &mut Gizmos<ToolGizmos>, at: Vec3, dir: Vec3, to_camera: Vec3, half: f32, color: Color) {
    let across = dir.cross(to_camera).normalize_or_zero();
    let across = if across == Vec3::ZERO { dir.any_orthonormal_vector() } else { across };
    gizmos.line(at - across * half, at + across * half, color);
}

/// A line along an axis from where the drag started, marked every 5 units, with the end where the object is now.
/// The long guide and the stretch travelled are laid end to end (never one over the other), so no line is
/// drawn on top of another and nothing can flicker between them, from any angle.
fn ruler(gizmos: &mut Gizmos<ToolGizmos>, r: &Ruler, moved: f32, to_camera: Vec3) {
    let reach = 5.0;
    let spans = gizmo::ruler_spans(moved, reach);
    let at = |s: f32| r.origin + r.dir * s;
    for (from, to) in spans.guide {
        if to > from {
            gizmos.line(at(from), at(to), bevy_color(r.rgb, 0.4));
        }
    }
    if spans.travelled.1 != spans.travelled.0 {
        gizmos.line(at(spans.travelled.0), at(spans.travelled.1), bevy_color(HIGHLIGHT_RGB, 1.0));
    }
    // Ticks across the line, towards the viewer's side so they read as a ruler.
    let small = r.size * 0.06;
    for mark in gizmo::ruler_marks(moved, 5.0, reach) {
        // (Where the object is now has its own, longer, tick: not two on the same spot.)
        if (mark - moved).abs() < 1e-3 && mark.abs() >= 1e-3 {
            continue;
        }
        let major = mark.abs() < 1e-3;
        let half = if major { small * 1.8 } else { small };
        cross_tick(gizmos, at(mark), r.dir, to_camera, half, if major { bevy_color(HIGHLIGHT_RGB, 1.0) } else { bevy_color(r.rgb, 0.9) });
    }
    // Where the object is now.
    cross_tick(gizmos, at(moved), r.dir, to_camera, small * 2.4, bevy_color(HIGHLIGHT_RGB, 1.0));
}

// ---- drawing: fills and text (egui overlay) -----------------------------------------------------------------------

fn project_points(view: &View, points: &[Vec3]) -> Option<Vec<egui::Pos2>> {
    points.iter().map(|p| view.project(*p).map(|s| egui::pos2(s.x, s.y))).collect()
}

/// Text with a dark plate behind it, readable on any background.
fn label(painter: &egui::Painter, at: egui::Pos2, text: &str, rgb: [u8; 3]) {
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::monospace(14.0), egui_color(rgb, 255));
    let rect = egui::Align2::LEFT_BOTTOM.anchor_size(at + egui::vec2(12.0, -10.0), galley.size());
    painter.rect_filled(rect.expand2(egui::vec2(5.0, 2.0)), 4.0, egui::Color32::from_rgba_unmultiplied(15, 15, 18, 215));
    painter.galley(rect.min, galley, egui_color(rgb, 255));
}

/// A disc on screen for a sphere at `world` with radius `r`.
fn disc(view: &View, world: Vec3, r: f32) -> Option<(egui::Pos2, f32)> {
    let c = view.project(world)?;
    let right = view.rot * Vec3::X;
    let e = view.project(world + right * r)?;
    Some((egui::pos2(c.x, c.y), (e - c).length().max(3.0)))
}

/// A solid cone, flat-shaded: the outline of its projected base circle and tip.
fn paint_cone(painter: &egui::Painter, view: &View, base: Vec3, tip: Vec3, radius: f32, fill: egui::Color32) {
    let points = gizmo::cone_points(base, tip, radius, 14);
    let Some(screen) = project_points(view, &points) else { return };
    let hull = gizmo::convex_hull(&screen.iter().map(|p| bevy::math::Vec2::new(p.x, p.y)).collect::<Vec<_>>());
    if hull.len() >= 3 {
        painter.add(egui::Shape::convex_polygon(hull.iter().map(|p| egui::pos2(p.x, p.y)).collect(), fill, egui::Stroke::NONE));
    }
}

/// The part of a ring that has been turned through, filled in: a pie slice from where the drag took hold
/// (`zero`, a direction in the ring's plane) round `axis` by `degrees`, either way and over a full turn
/// and more. It is a fan of triangles, so it is right however many turns it covers (a convex polygon is not).
fn paint_sector(painter: &egui::Painter, view: &View, origin: Vec3, axis: Vec3, zero: Vec3, radius: f32, degrees: f32, rgb: [u8; 3]) {
    let sweep = degrees.clamp(-360.0, 360.0);
    if sweep.abs() < 0.05 {
        return;
    }
    let steps = ((sweep.abs() / 3.0).ceil() as usize).clamp(2, 120);
    let mut points = vec![origin];
    points.extend((0..=steps).map(|k| origin + (Quat::from_axis_angle(axis, (sweep * k as f32 / steps as f32).to_radians()) * zero) * radius));
    let Some(screen) = project_points(view, &points) else { return };
    let fill = egui_color(rgb, 62);
    let mut mesh = egui::Mesh::default();
    for p in &screen {
        mesh.colored_vertex(*p, fill);
    }
    for k in 1..=steps as u32 {
        mesh.add_triangle(0, k, k + 1);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Draws the filled parts of the tool (or, while dragging, only the measurement text).
pub fn paint_overlay(painter: &egui::Painter, frame: &Frame, view: &View, state: &GizmoState) {
    if let Some(drag) = &state.drag {
        paint_measurement(painter, drag, view);
        return;
    }
    let lit = |h: Handle| state.hover == Some(h);
    let origin = frame.origin;
    match frame.tool {
        Tool::Select => {}
        Tool::Move | Tool::Pivot => {
            let signs = gizmo::facing_signs(view.pos, origin, &frame.dirs);
            for i in 0..3 {
                // The plane handles: small flat coloured squares (they are picked from a bigger area, see `gizmo::hit_move`).
                let c = gizmo::plane_corners(origin, &frame.dirs, &signs, i, frame.len);
                if let Some(points) = project_points(view, &c) {
                    let on = lit(Handle::Move(MoveHandle::Plane(i)));
                    let rgb = if on { HIGHLIGHT_RGB } else { AXIS_RGB[i] };
                    painter.add(egui::Shape::convex_polygon(points, egui_color(rgb, if on { 190 } else { 120 }), egui::Stroke::new(1.0_f32, egui_color(rgb, 210))));
                }
                // The arrowheads: solid cones.
                for sign in [1.0f32, -1.0] {
                    let (_, base, tip) = arrow_parts(frame, i, sign);
                    let on = lit(Handle::Move(MoveHandle::Arrow(i, sign)));
                    let rgb = if on { HIGHLIGHT_RGB } else { AXIS_RGB[i] };
                    let alpha = if sign == signs[i] { 255 } else { 160 };
                    paint_cone(painter, view, base, tip, frame.len * gizmo::CONE_RADIUS, egui_color(rgb, alpha));
                }
            }
            if frame.tool == Tool::Pivot {
                // The pivot itself: a ring with a cross, so it reads as a point rather than as a gizmo.
                if let Some(s) = view.project(origin) {
                    let c = egui::pos2(s.x, s.y);
                    let stroke = egui::Stroke::new(1.5_f32, egui_color(HIGHLIGHT_RGB, 255));
                    painter.circle_stroke(c, 7.0, stroke);
                    painter.line_segment([c - egui::vec2(11.0, 0.0), c + egui::vec2(11.0, 0.0)], stroke);
                    painter.line_segment([c - egui::vec2(0.0, 11.0), c + egui::vec2(0.0, 11.0)], stroke);
                    label(painter, c, "Pivot", HIGHLIGHT_RGB);
                }
            }
        }
        Tool::Scale => {
            if let Some(b) = frame.bounds {
                for axis in 0..3 {
                    for sign in [1.0f32, -1.0] {
                        let p = b.handle(view.pos, axis, sign);
                        if let Some((c, r)) = disc(view, p, gizmo::sphere_radius(view.pos, p)) {
                            let on = lit(Handle::Scale(axis, sign));
                            let rgb = if on { HIGHLIGHT_RGB } else { AXIS_RGB[axis] };
                            painter.circle(c, r, egui_color(rgb, if on { 255 } else { 225 }), egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(20, 20, 24, 200)));
                        }
                    }
                }
            }
        }
        Tool::Rotate => {
            // The trackball: a faint disc inside the rings, brighter when the mouse would grab it.
            if let Some((c, r)) = disc(view, origin, frame.len * 0.9) {
                let on = lit(Handle::Trackball);
                painter.circle(c, r, egui::Color32::from_rgba_unmultiplied(255, 255, 255, if on { 34 } else { 12 }), egui::Stroke::new(if on { 1.5_f32 } else { 0.0_f32 }, egui_color(HIGHLIGHT_RGB, 180)));
            }
        }
    }
}

fn paint_measurement(painter: &egui::Painter, drag: &Drag, view: &View) {
    let f = &drag.frame;
    match drag.handle {
        Handle::Move(MoveHandle::Arrow(axis, _)) => ruler_label(painter, view, f, axis, drag.done.moved[axis]),
        Handle::Move(MoveHandle::Plane(normal)) => {
            let (a, b) = gizmo::plane_axes(normal);
            ruler_label(painter, view, f, a, drag.done.moved[a]);
            ruler_label(painter, view, f, b, drag.done.moved[b]);
        }
        Handle::Ring(axis) => {
            let zero = drag.grab.normalize_or_zero();
            let radius = f.len * 0.9;
            paint_sector(painter, view, f.origin, f.dirs[axis], zero, radius, drag.done.angle, AXIS_RGB[axis]);
            let now = Quat::from_axis_angle(f.dirs[axis], drag.done.angle.to_radians()) * zero;
            if let Some(s) = view.project(f.origin + now * (radius * 1.2)) {
                label(painter, egui::pos2(s.x, s.y), &format!("{} {:+.1}\u{b0}", ["X", "Y", "Z"][axis], drag.done.angle), AXIS_RGB[axis]);
            }
        }
        Handle::Trackball => {
            if let Some(s) = view.project(f.origin) {
                label(painter, egui::pos2(s.x, s.y), &format!("Trackball {:.1}\u{b0}", drag.done.angle), HIGHLIGHT_RGB);
            }
        }
        Handle::Scale(axis, sign) => {
            let Some(b) = f.bounds else { return };
            // Where the grabbed face is now; the text is what the Move tool's is: one short value, in the axis's colour.
            if let Some(s) = view.project(b.face(axis, sign) + b.outward(axis, sign) * drag.done.pull) {
                label(painter, egui::pos2(s.x, s.y), &scale_text(&drag.done, axis), AXIS_RGB[axis]);
            }
        }
        Handle::Surface => {}
    }
}

/// What a scale drag shows: the new size along the axis being dragged, or with Shift the factor all three
/// were multiplied by.
fn scale_text(done: &Done, axis: usize) -> String {
    if done.proportional {
        format!("\u{d7}{:.2}", done.factor)
    } else {
        format!("{} {:.2}", ["X", "Y", "Z"][axis], done.extent)
    }
}

fn ruler_label(painter: &egui::Painter, view: &View, f: &Frame, axis: usize, moved: f32) {
    let now = f.origin + f.dirs[axis] * moved;
    let what = if f.tool == Tool::Pivot { "pivot " } else { "" };
    if let Some(s) = view.project(now) {
        label(painter, egui::pos2(s.x, s.y), &format!("{what}{} {:+.2}", ["X", "Y", "Z"][axis], moved), AXIS_RGB[axis]);
    }
    if let Some(s) = view.project(f.origin) {
        painter.circle_filled(egui::pos2(s.x, s.y), 3.5, egui_color(HIGHLIGHT_RGB, 255));
    }
}

/// The system that paints the overlay each frame (after the scene was synced and the tools ran).
pub fn draw_overlay(mut contexts: EguiContexts, ed: NonSend<Editor>, cam: Res<EditorCam>, vp: Res<ViewportRect>, state: Res<GizmoState>, mut metrics: Option<ResMut<Metrics>>) {
    let started = metrics.as_ref().and_then(|m| m.timings.start());
    let Ok(ctx) = contexts.ctx_mut() else { return };
    // Everything is clipped to the viewport so it never paints over a panel; the background layer
    // sits above the 3D scene and below windows and popups.
    let rect = egui::Rect::from_min_max(egui::pos2(vp.min_pt.x, vp.min_pt.y), egui::pos2(vp.max_pt.x, vp.max_pt.y));
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("tool_overlay"))).with_clip_rect(rect);
    let view = cam.view(&vp);

    // While dragging, the measurement stays where the drag began (the object moves under it), exactly as
    // the lines do; otherwise the handles follow the selection.
    if let Some(drag) = &state.drag {
        paint_overlay(&painter, &drag.frame, &view, &state);
    } else if let Some(frame) = active_frame(&ed, &cam, state.part) {
        paint_overlay(&painter, &frame, &view, &state);
    }
    crate::viewport::paint_speed_readout(&painter, &ed, rect);
    if let Some(m) = metrics.as_deref_mut() {
        m.timings.stop(timing::GIZMOS, started);
    }
}

/// The active tool's frame for the selection, if there is anything to show. With the Scale tool and Alt
/// held, `part` is the part of a selected model the handles are on instead of the whole selection.
pub fn active_frame(ed: &Editor, cam: &EditorCam, part: Option<Id>) -> Option<Frame> {
    if ed.play.as_ref().is_some_and(|p| p.game_camera) {
        return None;
    }
    let dm = ed.dm.borrow();
    if let (Tool::Scale, Some(part)) = (ed.tool, part.filter(|p| dm.exists(*p))) {
        return frame_for(&dm, Tool::Scale, ed.settings.space, part, &[part], cam.pos);
    }
    let active = ed.selected?;
    let targets = ed.top_level_selection();
    dm.exists(active).then_some(())?;
    frame_for(&dm, ed.tool, ed.settings.space, active, &targets, cam.pos)
}

/// The system that draws the tools' lines.
pub fn draw_tool_lines(mut gizmos: Gizmos<ToolGizmos>, ed: NonSend<Editor>, cam: Res<EditorCam>, vp: Res<ViewportRect>, state: Res<GizmoState>, mut metrics: Option<ResMut<Metrics>>) {
    let started = metrics.as_ref().and_then(|m| m.timings.start());
    let view = cam.view(&vp);
    // While dragging, keep the frame the drag started with (the object moves under it).
    if let Some(drag) = &state.drag {
        draw_lines(&mut gizmos, &drag.frame, &view, &state);
    } else if let Some(frame) = active_frame(&ed, &cam, state.part) {
        draw_lines(&mut gizmos, &frame, &view, &state);
    }
    if let Some(m) = metrics.as_deref_mut() {
        m.timings.stop(timing::GIZMOS, started);
    }
}

#[cfg(test)]
mod tests;
