//! The 3D viewport: mirrors the data model into Bevy entities (through the shared renderer), drives
//! the editor camera, and turns mouse input into selecting things and using the transform tools.
//!
//! Picking and gizmo hit-testing are done in plain math from the editor camera's own pose and the
//! viewport rectangle (in egui points, the same units as the pointer position), so they are
//! independent of Bevy's camera bookkeeping and of the display's DPI scaling. The tools themselves
//! (`tools`, `gizmo`) and the shortcut settings (`keymap`) live elsewhere.
//!
//! **Mouse priorities** (a click is handled by the first of these that applies):
//! 1. a handle of the active tool under the mouse (unless Shift or Alt is held, which mean "select"; the Scale
//!    tool is the exception: there Shift and Ctrl are modifiers of the drag, and Alt picks one part of a model);
//! 2. selecting: plain click picks, Shift toggles, Alt cycles through what is behind, Shift+Alt does both. With
//!    the Select tool a plain press on an object that then moves the mouse (a few points, so a click that
//!    wobbles is still a click) picks it up and slides it, see `tools::Press`;
//! 3. the right mouse button flies / looks (or, with Alt held, orbits), independent of the above.

use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    camera::{visibility::RenderLayers, CameraOutputMode, ClearColorConfig, Viewport},
    render::render_resource::BlendState,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use bevy_egui::{egui, PrimaryEguiContext};
use forge_engine::{
    core::GizmoShape,
    diag::{timed, timing, Metrics},
    dm::{DataModel, Id},
    physics::{pick_collider, pick_colliders_all, raycast, RayParams},
    render::{self, Mode, SpawnParams, SyncOptions},
};

use crate::{
    gizmo::View,
    keymap::Action,
    settings::{ZoomMode, MAX_FLY_SPEED, MAX_SECOND_SPEED, MIN_FLY_SPEED, MIN_SECOND_SPEED},
    state::{Editor, SpeedKind},
    tools::{self, GizmoState, Handle, Press, Snap},
};

#[derive(Component)]
pub struct EditorCamera;

/// Screen region left over by the egui panels. Written by the UI every frame.
#[derive(Resource, Default)]
pub struct ViewportRect {
    /// Physical pixels (for the camera viewport).
    pub min_px: Vec2,
    pub max_px: Vec2,
    /// egui points (same space as `pointer`).
    pub min_pt: Vec2,
    pub max_pt: Vec2,
    pub pointer: Option<Vec2>,
    pub hovered: bool,
    /// A text field has keyboard focus (keys must not reach the game).
    pub typing: bool,
    pub valid: bool,
}

/// Orbit-style view: the camera circles a point instead of turning in place.
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    pub pivot: Vec3,
    pub dist: f32,
    /// The object it is centred on; selecting something else leaves orbit mode.
    pub target: Option<Id>,
}

/// What holding the right mouse button is doing right now.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Look {
    Idle,
    /// Turning the camera in place.
    Free,
    /// Orbiting a fixed point for the length of the drag (Alt + right mouse).
    Pivot(Vec3),
    /// Orbiting the focused object (after Focus).
    Focused,
}

#[derive(Resource)]
pub struct EditorCam {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
    look: Look,
    /// The persistent orbit set by Focus.
    pub orbit: Option<Orbit>,
    /// A smooth move towards a focus position: (from, to, progress 0..1).
    glide: Option<(Vec3, Vec3, f32)>,
    /// The mouse is captured (hidden, kept in the viewport) for a look / orbit drag.
    pub captured: bool,
    /// Zoom the wheel asked for that has not been travelled yet (positive = closer), eased out over a few frames.
    zoom_pending: f32,
    /// The direction of that travel (unit length).
    zoom_dir: Vec3,
}

impl Default for EditorCam {
    fn default() -> Self {
        Self { pos: Vec3::new(0.0, 14.0, 30.0), yaw: 0.0, pitch: -0.3, fov: 70f32.to_radians(), look: Look::Idle, orbit: None, glide: None, captured: false, zoom_pending: 0.0, zoom_dir: Vec3::NEG_Z }
    }
}

impl EditorCam {
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }

    /// The camera as the tools see it (for rays and for projecting handles onto the screen).
    pub fn view(&self, vp: &ViewportRect) -> View {
        View { pos: self.pos, rot: self.rotation(), fov: self.fov, min: vp.min_pt, size: vp.max_pt - vp.min_pt }
    }

    /// No right-mouse drag is in progress.
    pub fn is_idle(&self) -> bool {
        self.look == Look::Idle
    }

    /// How far the camera still has to travel for the zoom that was asked for (0 once it has arrived).
    #[cfg(test)]
    pub fn zoom_remaining(&self) -> f32 {
        self.zoom_pending
    }

    /// Points the camera at `target` from `dist` away without changing where it looks.
    fn place_around(&mut self, pivot: Vec3, dist: f32) {
        self.pos = pivot - self.rotation() * Vec3::NEG_Z * dist;
    }
}

pub fn spawn_editor_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Msaa::Sample4,
        Projection::Perspective(PerspectiveProjection { fov: 70f32.to_radians(), near: 0.1, far: 10_000.0, ..default() }),
        Transform::from_xyz(0.0, 14.0, 30.0),
        EditorCamera,
    ));
    // egui's own camera (the setup bevy_egui's own side-panel example uses): it covers the whole window and draws after the
    // 3D view, so the panels and menus are drawn over the picture. (The 3D camera only covers the viewport panel.)
    // Its texture starts every frame transparent (`clear_color`) and is blended over what is already in the window
    // (`output_mode`): the viewport panel is see-through, so anything left in the texture from the last frame (a tool's
    // handles, a label) would stay over the 3D view and smear as the camera moves. `RenderLayers::none()` keeps it from
    // drawing anything but the UI.
    //
    // `Msaa::Off` is what makes the clearing hold: egui draws into the camera's resolved texture, which an antialiased
    // camera does not clear each frame (the 3D view smeared the handles of the move tool across the screen until this
    // was set). egui has no use for multisampling anyway - it antialiases its own shapes.
    commands.spawn((
        Camera2d,
        PrimaryEguiContext,
        RenderLayers::none(),
        Msaa::Off,
        Camera {
            order: 100,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            output_mode: CameraOutputMode::Write { blend_state: Some(BlendState::ALPHA_BLENDING), clear_color: ClearColorConfig::None },
            ..default()
        },
    ));
}
pub fn sync_scene(ed: NonSend<Editor>, time: Res<Time>, mut commands: Commands, mut spawn: SpawnParams, mut metrics: Option<ResMut<Metrics>>) {
    let dm = ed.dm.clone();
    let mut dm = dm.borrow_mut();
    // A playing game looks like the real one (hidden parts stay hidden); the editor keeps its own camera.
    let mode = if ed.playing() { Mode::Play } else { Mode::Editor };
    // The project's model scale (studs per file unit) is what models are read at.
    let scale = ed.project.as_ref().map_or(1.0, |p| p.manifest.assets.mesh_scale());
    if spawn.cache.mesh_scale != scale {
        spawn.cache.mesh_scale = scale;
        spawn.cache.invalidate();
    }
    // A model, picture, material or sound file changed: read the files again and redraw every part (ender::sync redraws
    // them when the cache says its files were forgotten).
    if ed.reload_meshes.take() {
        spawn.cache.invalidate();
    }
    timed(metrics.as_deref_mut(), timing::SCENE_SYNC, || render::sync(&mut dm, &mut commands, &mut spawn, SyncOptions { mode, msaa: Msaa::Sample4, dt: time.delta_secs(), camera: Some(ed.cam_ray.0) }, &ed.source));
}

/// Finishes an asset drop on the viewport: finds what is under the pointer and lets the editor use the
/// assets there (create an object where the ray lands, or apply them to the object it hits).
pub fn drop_assets(vp: Res<ViewportRect>, cam: Res<EditorCam>, mut ed: NonSendMut<Editor>) {
    let Some(drop) = ed.pending_drop.take() else { return };
    let view = cam.view(&vp);
    let Some((origin, dir)) = view.ray(drop.pointer) else { return };
    let hit = raycast(&ed.dm.borrow(), origin, dir * 1000.0, &RayParams::default());
    let (target, at) = match hit {
        Some(h) => (Some(h.id), (h.pos + Vec3::Y * 0.5).round()),
        None => (None, (origin + dir * 25.0).round()),
    };
    ed.drop_assets(&drop.paths, target, Some(at));
}

/// The game's camera is in control of the viewport (the editor's fly camera and tools step aside).
fn game_camera_active(ed: &Editor) -> bool {
    ed.play.as_ref().is_some_and(|p| p.game_camera)
}

/// Centre and radius of the sphere around the collision volumes (or pivots) of some instances.
pub fn bounds_of(dm: &DataModel, ids: &[Id]) -> Option<(Vec3, f32)> {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for id in ids {
        for n in dm.subtree(*id) {
            if let Some(info) = dm.collider(n) {
                let (a, b) = info.aabb();
                min = min.min(a);
                max = max.max(b);
            }
        }
    }
    if min.x <= max.x {
        return Some(((min + max) * 0.5, ((max - min) * 0.5).length().max(0.5)));
    }
    // Nothing solid: the pivots.
    let pivots: Vec<Vec3> = ids.iter().filter_map(|id| dm.get_pivot(*id).map(|(p, _)| p)).collect();
    if pivots.is_empty() {
        return None;
    }
    let center = pivots.iter().copied().sum::<Vec3>() / pivots.len() as f32;
    Some((center, 2.0))
}

/// How far from a sphere of `radius` the camera must be for it to fill the view (the narrower of
/// the two field-of-view angles decides, so tall and wide windows both frame it).
pub fn frame_distance(radius: f32, fov: f32, aspect: f32) -> f32 {
    let half_v = fov * 0.5;
    let half_h = (half_v.tan() * aspect.max(0.1)).atan();
    (radius / half_v.min(half_h).sin() * 1.15).clamp(3.0, 5000.0)
}

/// What is in front of the camera along a ray: how far away the first thing the ray hits is (`true`), else where it
/// meets the ground, else (`false`) a distance that suits how high the camera is, so zooming and orbiting still have
/// somewhere sensible to go in empty space.
fn reach_along(dm: &DataModel, origin: Vec3, dir: Vec3) -> (f32, bool) {
    if let Some((_, t)) = pick_collider(dm, origin, dir) {
        return (t, true);
    }
    if dir.y < -1e-3 {
        let t = -origin.y / dir.y;
        if t > 0.0 && t < 2000.0 {
            return (t, true);
        }
    }
    (OPEN_REACH.max(origin.y.abs() * 0.5), false)
}

/// The point the camera orbits for an Alt + right-mouse drag: what is under the mouse (the first thing the ray
/// through it hits, else the ground, else a point ahead of the camera). The selection plays no part.
fn orbit_point(ed: &Editor, cam: &EditorCam, vp: &ViewportRect) -> Vec3 {
    let dm = ed.dm.borrow();
    let view = cam.view(vp);
    let (origin, dir) = vp.pointer.and_then(|p| view.ray(p)).unwrap_or((cam.pos, view.forward()));
    let (t, _) = reach_along(&dm, origin, dir);
    origin + dir * t.max(ORBIT_MIN_DISTANCE)
}

/// Which way the wheel moves the camera, how far ahead there is to move, and whether that is a surface that must
/// not be passed. In orbit mode (after Focus) it is toward the focus point; otherwise it depends on the zoom mode
/// of the editor settings: along the view (`Camera`) or along the ray through the mouse (`Hover`, which falls back
/// to the view when the pointer is nowhere). The selection is never consulted.
fn zoom_target(ed: &Editor, cam: &EditorCam, vp: &ViewportRect) -> (Vec3, f32, bool) {
    let view = cam.view(vp);
    if let Some(o) = cam.orbit {
        return (view.forward(), o.dist, true);
    }
    let dm = ed.dm.borrow();
    let dir = match ed.settings.zoom_mode {
        ZoomMode::Hover => vp.pointer.and_then(|p| view.ray(p)).map_or(view.forward(), |r| r.1),
        ZoomMode::Camera => view.forward(),
    };
    let (reach, blocked) = reach_along(&dm, cam.pos, dir);
    (dir, reach, blocked)
}
/// Zoom: each wheel notch covers this fraction of the distance to what is ahead (so it slows down toward a surface and
/// speeds up away from it), but never less than `ZOOM_MIN_STEP` nor more than `ZOOM_MAX_STEP` studs per notch.
const ZOOM_STEP: f32 = 0.10;
const ZOOM_MIN_STEP: f32 = 0.2;
const ZOOM_MAX_STEP: f32 = 150.0;
/// Zooming in stops this far from a surface (it can be left again by zooming out or flying).
const ZOOM_STOP_DISTANCE: f32 = 0.5;
/// How quickly the zoom that was asked for is travelled: the share of what is left that is covered per second
/// is `1 - e^-ZOOM_RATE`. About 99 % of it is done in a third of a second.
const ZOOM_RATE: f32 = 14.0;
/// How far ahead the camera assumes there is something when nothing is (empty space).
const OPEN_REACH: f32 = 20.0;
/// The nearest an orbit pivot may be to the camera.
const ORBIT_MIN_DISTANCE: f32 = 0.5;

fn alt_down(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight)
}

fn shift_down(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
}

fn ctrl_down(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
}

#[allow(clippy::too_many_arguments)]
pub fn camera_controls(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    vp: Res<ViewportRect>,
    gizmo: Res<GizmoState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut ed: NonSendMut<Editor>,
    mut cam: ResMut<EditorCam>,
    mut cameras: Query<&mut Transform, With<EditorCamera>>,
) {
    if game_camera_active(&ed) {
        cam.look = Look::Idle;
        cam.captured = false;
        return; // `play::apply_play_camera` positions the camera from the game
    }
    let dt = time.delta_secs();
    let focused = windows.single().map(|w| w.focused).unwrap_or(true);

    // ---- the right mouse button: look around, or (with Alt) orbit --------------------------------------
    if mouse.just_pressed(MouseButton::Right) && vp.hovered && !gizmo.dragging() {
        cam.look = if alt_down(&keys) {
            // The pivot is what is under the mouse when the drag begins, and stays there however the camera
            // moves: the camera turns about it from exactly where it and its view are now.
            let pivot = orbit_point(&ed, &cam, &vp);
            cam.orbit = None;
            cam.zoom_pending = 0.0;
            Look::Pivot(pivot)
        } else if cam.orbit.is_some() {
            Look::Focused
        } else {
            Look::Free
        };
        cam.captured = true;
    }
    if !mouse.pressed(MouseButton::Right) || !focused {
        cam.look = Look::Idle;
        cam.captured = false;
    }
    if cam.look != Look::Idle {
        let before = cam.rotation();
        cam.yaw -= motion.delta.x * 0.003;
        cam.pitch = (cam.pitch - motion.delta.y * 0.003).clamp(-1.54, 1.54);
        match cam.look {
            // The whole camera swings about the pivot by exactly the turn of the view, so the pivot stays where it
            // is on the screen (under the mouse, wherever that was) and nothing jumps when the drag begins.
            Look::Pivot(pivot) => {
                let turn = cam.rotation() * before.inverse();
                cam.pos = pivot + turn * (cam.pos - pivot);
            }
            Look::Focused => {
                if let Some(o) = cam.orbit {
                    cam.place_around(o.pivot, o.dist);
                }
            }
            _ => {}
        }
    }
    let rot = cam.rotation();

    // Selecting something else leaves orbit mode (the camera was centred on the old object).
    if cam.orbit.is_some_and(|o| o.target != ed.selected) {
        cam.orbit = None;
    }

    // ---- flying: works whenever the mouse is over the viewport (the right button only adds looking) -----
    let flying_allowed = matches!(cam.look, Look::Idle | Look::Free) && cam.glide.is_none();
    if flying_allowed {
        let keys = &ed.keys;
        let mut dir = Vec3::ZERO;
        for (action, v) in [
            (Action::CamForward, rot * Vec3::NEG_Z),
            (Action::CamBack, rot * Vec3::Z),
            (Action::CamLeft, rot * Vec3::NEG_X),
            (Action::CamRight, rot * Vec3::X),
            (Action::CamUp, Vec3::Y),
            (Action::CamDown, Vec3::NEG_Y),
        ] {
            if keys.held(action) {
                dir += v;
            }
        }
        if dir != Vec3::ZERO {
            let boost = if keys.held(Action::FastCamera) { ed.settings.fast_multiplier } else { 1.0 };
            cam.pos += dir.normalize_or_zero() * ed.settings.camera_speed * boost * dt;
            cam.orbit = None; // moving by hand leaves orbit mode
        }
    }

    // ---- the wheel ------------------------------------------------------------------------------------------
    // The speed wheels also work while the camera is being flown with the pointer somewhere else; zooming needs the
    // pointer over the picture.
    let wheel = if vp.hovered || ed.nav_latched {
        let d = if scroll.delta.y != 0.0 { scroll.delta.y } else { scroll.delta.x };
        match scroll.unit {
            MouseScrollUnit::Line => d,
            MouseScrollUnit::Pixel => d / 40.0,
        }
    } else {
        0.0
    };
    if wheel != 0.0 {
        match ed.keys.wheel {
            Some(Action::FastSpeedWheel) => {
                // Each notch changes the second speed by 10 %; it may go below 1x (slow, precise movement).
                ed.settings.fast_multiplier = (ed.settings.fast_multiplier * 1.1f32.powf(wheel)).clamp(MIN_SECOND_SPEED, MAX_SECOND_SPEED);
                ed.settings.touch();
                ed.speed_hud = Some((SpeedKind::Second, std::time::Instant::now() + SPEED_HUD_TIME));
            }
            Some(Action::PrimarySpeedWheel) => {
                ed.settings.camera_speed = (ed.settings.camera_speed * 1.1f32.powf(wheel)).clamp(MIN_FLY_SPEED, MAX_FLY_SPEED);
                ed.settings.touch();
                ed.speed_hud = Some((SpeedKind::Fly, std::time::Instant::now() + SPEED_HUD_TIME));
            }
            Some(Action::ZoomWheel) if vp.hovered => {
                // Where to, how far there is to go, and whether a surface is in the way (never the selection).
                let (dir, reach, blocked) = zoom_target(&ed, &cam, &vp);
                let notches = wheel.clamp(-6.0, 6.0);
                // Toward a surface each notch closes a tenth of what is left (so n notches in and n notches out are
                // exactly back where they started); in open space every notch is the same length. Never a tiny
                // step, never a huge one.
                let wanted = if blocked { reach * (1.0 - (1.0 - ZOOM_STEP).powf(notches)) } else { reach * ZOOM_STEP * notches };
                let mut travel = wanted.signum() * wanted.abs().clamp(ZOOM_MIN_STEP * notches.abs(), ZOOM_MAX_STEP * notches.abs());
                if blocked && travel > 0.0 {
                    // Closer, but not through what is ahead (counting what is already on its way).
                    travel = travel.min((reach - ZOOM_STOP_DISTANCE - cam.zoom_pending.max(0.0)).max(0.0));
                }
                if cam.zoom_pending != 0.0 && cam.zoom_dir.dot(dir) < 0.9 {
                    cam.zoom_pending = 0.0; // the view or the pointer moved on: the old way is forgotten
                }
                cam.zoom_dir = dir;
                cam.zoom_pending += travel;
            }
            _ => {}
        }
    }
    // The zoom asked for is travelled in a few frames, fast at first and gently at the end.
    if cam.zoom_pending != 0.0 {
        let share = 1.0 - (-ZOOM_RATE * dt).exp();
        let mut step = cam.zoom_pending * share;
        if cam.zoom_pending.abs() < 5e-4 {
            step = cam.zoom_pending; // the last crumb
        }
        cam.zoom_pending -= step;
        if let Some(o) = cam.orbit.as_mut() {
            // (Zooming in orbit mode changes the distance to the focus point.)
            o.dist = (o.dist - step).clamp(ORBIT_MIN_DISTANCE, 5000.0);
            let (pivot, dist) = (o.pivot, o.dist);
            cam.place_around(pivot, dist);
        } else {
            let dir = cam.zoom_dir;
            cam.pos += dir * step;
        }
    }
    // ---- focus ---------------------------------------------------------------------------------------------
    if ed.focus_request {
        ed.focus_request = false;
        let target = bounds_of(&ed.dm.borrow(), &ed.selection_ids());
        if let Some((center, radius)) = target {
            cam.zoom_pending = 0.0;
            let aspect = (vp.max_pt.x - vp.min_pt.x) / (vp.max_pt.y - vp.min_pt.y).max(1.0);
            let dist = frame_distance(radius, cam.fov, aspect);
            let to = center - rot * Vec3::NEG_Z * dist;
            cam.glide = Some((cam.pos, to, 0.0));
            // From now on the camera orbits the focused object.
            cam.orbit = Some(Orbit { pivot: center, dist, target: ed.selected });
        }
    }
    if let Some((from, to, t)) = cam.glide {
        let t = (t + dt / 0.22).min(1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        cam.pos = from.lerp(to, eased);
        cam.glide = (t < 1.0).then_some((from, to, t));
    }
    ed.cam_ray = (cam.pos, rot * Vec3::NEG_Z);

    for mut t in &mut cameras {
        t.translation = cam.pos;
        t.rotation = rot;
    }
}

/// Hides the pointer and keeps it from moving while the right button is held in the viewport, so a look or
/// orbit drag is relative motion that never runs into the edge of the screen.
///
/// The camera turns by the mouse's *device* motion (`AccumulatedMouseMotion`), which does not depend on where
/// the pointer is; all this has to do is stop the pointer from wandering. It is put back where the button went
/// down - the spot it is already at - after every frame's motion, so nothing is seen to jump (an earlier version
/// sent it to the middle of the viewport, which showed as a snap the moment the look began) and, since it never
/// leaves the viewport, the viewport keeps the hover for the whole drag. Where the platform can lock the pointer
/// (macOS, Wayland) the lock holds it instead and the resets do nothing. On release (or when the window loses
/// focus: `camera_controls` ends the drag then) the pointer is shown again, in the same place.
pub fn viewport_cursor(cam: Res<EditorCam>, mut windows: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>, mut was: Local<Option<Option<Vec2>>>) {
    let Ok((mut window, mut cursor)) = windows.single_mut() else { return };
    match (cam.captured, was.is_some()) {
        (true, false) => {
            *was = Some(window.cursor_position());
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
        (false, true) => {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
            if let Some(Some(back)) = was.take() {
                window.set_cursor_position(Some(back));
            }
        }
        _ => {}
    }
    if let (true, Some(Some(origin))) = (cam.captured, *was) {
        window.set_cursor_position(Some(origin));
    }
}

/// Screen points the mouse has to move with the button down before an object that was pressed is picked up:
/// a click that wobbles a little still only selects.
pub const DRAG_START_DISTANCE: f32 = 5.0;

/// Whether the part of a selected model the Scale tool's handles are on (Alt) is what is under the mouse now,
/// and which part that is. The choice sticks while Alt is held, so the mouse can travel from the part to its
/// handles, which stand off from its faces, without the handles letting go of it.
fn update_part_override(ed: &Editor, cam: &EditorCam, view: &View, pointer: Option<Vec2>, alt: bool, can_pick: bool, state: &mut GizmoState) {
    if ed.tool != crate::state::Tool::Scale || !alt {
        state.part = None;
        return;
    }
    let dm = ed.dm.borrow();
    let targets = ed.top_level_selection();
    let inside = |id: Id| targets.iter().any(|t| *t != id && dm.is_descendant_of(id, *t));
    state.part = state.part.filter(|p| dm.exists(*p) && inside(*p));
    let (true, Some(pointer)) = (can_pick, pointer) else { return };
    let Some(ray) = view.ray(pointer) else { return };
    // The handles of the part already chosen are under the mouse: keep it.
    if let Some(part) = state.part {
        let held = tools::frame_for(&dm, crate::state::Tool::Scale, ed.settings.space, part, &[part], cam.pos).and_then(|f| tools::hit(f.tool, &f, view, pointer));
        if held.is_some() {
            return;
        }
    }
    // Otherwise the nearest part of the selection under the mouse (something solid in front of it that is
    // not part of the selection does not stop it being chosen: Alt is asking for that part).
    let chosen = pick_colliders_all(&dm, ray.0, ray.1).into_iter().map(|(id, _)| id).find(|id| inside(*id) && dm.get(*id).is_some_and(|i| i.class.prop("Size").is_some()));
    if chosen.is_some() {
        state.part = chosen;
    }
}

/// Selecting and the transform tools.
#[allow(clippy::too_many_arguments)]
pub fn pick_and_move(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    vp: Res<ViewportRect>,
    cam: Res<EditorCam>,
    mut ed: NonSendMut<Editor>,
    mut state: ResMut<GizmoState>,
) {
    if game_camera_active(&ed) {
        *state = GizmoState::default();
        ed.pending_turns.clear();
        ed.drag_active = false;
        return; // clicks belong to the game
    }
    ed.normalize_selection();
    // Quarter turns (Rotate 90 / Flip) pressed while dragging go into the drag; with no drag any more (it ended
    // since the key was pressed) they are an ordinary turn.
    for axis in std::mem::take(&mut ed.pending_turns) {
        ed.turn_selection(axis, state.drag.as_mut());
    }
    pick_and_move_frame(&mouse, &keys, &vp, &cam, &mut ed, &mut state);
    // From here to the next frame's shortcuts: is a drag going on? (The shortcut handler asks.)
    ed.drag_active = state.drag.is_some();
}

fn pick_and_move_frame(mouse: &ButtonInput<MouseButton>, keys: &ButtonInput<KeyCode>, vp: &ViewportRect, cam: &EditorCam, ed: &mut Editor, state: &mut GizmoState) {
    let (ed, state) = (&mut *ed, &mut *state);
    let view = cam.view(&vp);
    let pointer = vp.pointer;
    let ray = pointer.and_then(|p| view.ray(p));
    // The modifier keys are read afresh every frame, drag or not: pressing or releasing one mid-drag takes
    // effect on the next frame.
    let (shift, alt, ctrl) = (shift_down(&keys), alt_down(&keys), ctrl_down(&keys));
    let (move_increment, rotate_increment) = (ed.settings.move_increment, ed.settings.rotate_increment);

    // A drag in progress.
    if let Some(mut drag) = state.drag.take() {
        if mouse.pressed(MouseButton::Left) {
            if let Some(r) = ray {
                // Ctrl ignores the snapping increments, except when scaling, where it means "about the centre".
                let scaling = matches!(drag.handle(), Handle::Scale(..));
                // (Ctrl taking hold for a shortcut, Ctrl+R for one, must not move what is carried: see `ctrl_effective`.)
                let free = !scaling && drag.ctrl_effective(ctrl, pointer);
                let snap = Snap { move_step: if free { 0.0 } else { move_increment }, turn_step: if free { 0.0 } else { rotate_increment }, proportional: shift, from_center: ctrl };
                let changed = tools::update_drag(&mut ed.dm.borrow_mut(), &mut drag, r, &snap);
                if changed {
                    ed.dirty = true;
                }
            }
            state.drag = Some(drag);
        }
        return;
    }
    let free = ctrl;
    let snap = Snap { move_step: if free { 0.0 } else { move_increment }, turn_step: if free { 0.0 } else { rotate_increment }, ..Snap::default() };

    // An object that was pressed: a click, or the start of moving it.
    if let Some(press) = state.press.take() {
        if !mouse.pressed(MouseButton::Left) {
            if press.collapse {
                ed.select_only(Some(press.id));
                ed.reveal = true;
            }
            return;
        }
        let moved = pointer.map_or(0.0, |p| p.distance(press.pointer));
        let Some(r) = ray.filter(|_| moved >= DRAG_START_DISTANCE) else {
            state.press = Some(press);
            return;
        };
        let targets = ed.top_level_selection();
        let begun = ed.selected.and_then(|active| tools::begin_drag(&ed.dm.borrow(), Handle::Surface, tools::surface_frame(press.point), press.ray, &targets, active));
        if let Some(mut drag) = begun {
            drag.snap_step = snap.turn_step;
            drag.begin_with_ctrl(ctrl);
            if tools::update_drag(&mut ed.dm.borrow_mut(), &mut drag, r, &snap) {
                ed.dirty = true;
            }
            state.drag = Some(drag);
        }
        return;
    }

    let can_pick = vp.hovered && cam.is_idle();
    update_part_override(ed, cam, &view, pointer, alt, can_pick, state);

    // What is under the mouse: a handle of the tool, if there is one.
    let frame = tools::active_frame(&ed, &cam, state.part);
    state.hover = match (frame, pointer) {
        (Some(f), Some(p)) if can_pick => tools::hit(f.tool, &f, &view, p),
        _ => None,
    };

    if !(mouse.just_pressed(MouseButton::Left) && can_pick) {
        return;
    }
    let Some(r) = ray else { return };

    // Choosing a parent (Properties > Parent): this click picks the object under the mouse, and nothing else.
    // (Choosing a model's primary part works the same way, and a click on something that cannot be one says why.)
    if ed.picking_any() {
        let hits: Vec<Id> = pick_colliders_all(&ed.dm.borrow(), r.0, r.1).into_iter().map(|(id, _)| id).collect();
        let hit = hits.iter().copied().find(|id| ed.pick_ok(*id)).or_else(|| hits.first().copied().filter(|_| ed.picking_primary.is_some()));
        if let Some(id) = hit {
            ed.pick_candidate(id);
        }
        return;
    }

    // 1. A handle. Shift and Alt mean "select", so they pass through to the objects behind the handle; except
    // with the Scale tool, where Shift is "keep the proportions" and Alt, once it has chosen a part, is that part's handles.
    let passes_through = match ed.tool {
        crate::state::Tool::Scale => alt && state.part.is_none(),
        _ => shift || alt,
    };
    if !passes_through {
        if let (Some(handle), Some(f)) = (state.hover, frame) {
            // The tools act on every selected object (the pivot tool only on the active one); the Scale tool
            // with a part chosen acts on that part alone.
            let (targets, active) = match state.part {
                Some(part) if ed.tool == crate::state::Tool::Scale => (vec![part], Some(part)),
                _ => (ed.top_level_selection(), ed.selected),
            };
            if let Some(active) = active {
                let begun = tools::begin_drag(&ed.dm.borrow(), handle, f, r, &targets, active);
                if let Some(mut drag) = begun {
                    drag.snap_step = snap.turn_step;
            drag.begin_with_ctrl(ctrl);
                    state.drag = Some(drag);
                    return;
                }
            }
        }
    }

    // 2. Selecting: everything under the mouse, nearest first.
    let hits = pick_colliders_all(&ed.dm.borrow(), r.0, r.1);
    let ids: Vec<Id> = hits.iter().map(|(id, _)| *id).collect();
    let at = pointer.unwrap_or(Vec2::ZERO);
    // With the Select tool a press on an object can also become picking it up; see `Press`.
    if let (crate::state::Tool::Select, false, false, Some(&(id, t))) = (ed.tool, shift, alt, hits.first()) {
        // (A part of a group is the group: that is what is picked up.)
        let id = ed.pick_target(id);
        let one_of_several = ed.selection.len() > 1 && ed.is_selected(id);
        if one_of_several {
            ed.pick_cycle.reset();
        } else {
            ed.click_select(&ids, at, false, false);
        }
        state.press = Some(Press { pointer: at, id, point: r.0 + r.1 * t, ray: r, collapse: one_of_several });
        return;
    }
    ed.click_select(&ids, at, shift, alt);
}
pub fn apply_viewport(vp: Res<ViewportRect>, windows: Query<&Window, With<PrimaryWindow>>, mut cameras: Query<&mut Camera, With<EditorCamera>>) {
    let Ok(window) = windows.single() else { return };
    let size = UVec2::new(window.physical_width(), window.physical_height());
    // With no viewport panel on screen (hidden, or behind another tab) there is nothing to draw the 3D view into.
    for mut camera in &mut cameras {
        camera.is_active = vp.valid;
    }
    if !vp.valid || size.x < 2 || size.y < 2 {
        return;
    }
    let min = vp.min_px.max(Vec2::ZERO).as_uvec2().min(size - UVec2::ONE);
    let extent = vp.max_px.max(Vec2::ZERO).as_uvec2().min(size).saturating_sub(min).max(UVec2::ONE);
    for mut camera in &mut cameras {
        camera.viewport = Some(Viewport { physical_position: min, physical_size: extent.min(size - min), ..default() });
    }
}

/// The ground grid and the outlines of the selection. (The tools' handles are in `tools`.)
pub fn draw_gizmos(mut gizmos: Gizmos, ed: NonSend<Editor>, mut metrics: Option<ResMut<Metrics>>) {
    let started = metrics.as_ref().and_then(|m| m.timings.start());
    // (Drawing only: the tools' snapping works on `move_increment`, not on these lines.)
    if ed.settings.render.show_grid {
        let minor = Color::srgba(1.0, 1.0, 1.0, 0.06);
        for i in -16..=16 {
            let f = i as f32 * 16.0;
            let (cz, cx) = if i == 0 { (Color::srgba(0.3, 0.5, 1.0, 0.5), Color::srgba(1.0, 0.3, 0.3, 0.5)) } else { (minor, minor) };
            gizmos.line(Vec3::new(f, 0.02, -256.0), Vec3::new(f, 0.02, 256.0), cz);
            gizmos.line(Vec3::new(-256.0, 0.02, f), Vec3::new(256.0, 0.02, f), cx);
        }
    }

    let dm = ed.dm.borrow();
    let mut budget = 400usize;
    for sel in ed.selection_ids() {
        // The active object is outlined in orange, the others in a paler yellow.
        let active = ed.selected == Some(sel);
        let outline = if active { Color::srgb(1.0, 0.65, 0.1) } else { Color::srgb(0.95, 0.9, 0.45) };

        // Each component of each selected object decides what to draw (collision box, light range, ...).
        let mut shapes: Vec<GizmoShape> = Vec::new();
        for n in dm.subtree(sel).into_iter().take(budget) {
            budget = budget.saturating_sub(1);
            let Some(class) = dm.get(n).map(|i| i.class) else { continue };
            for comp in &class.components {
                if let Some(gizmo) = comp.gizmo {
                    gizmo(&dm, n, &mut shapes);
                }
            }
        }
        for shape in shapes {
            match shape {
                GizmoShape::Box { center, rotation, size } => {
                    gizmos.cube(Transform { translation: center, rotation, scale: size }, outline);
                }
                GizmoShape::Sphere { center, radius } => {
                    gizmos.sphere(Isometry3d::from_translation(center), radius, Color::srgba(1.0, 0.9, 0.4, 0.5));
                }
                GizmoShape::Arrow { from, to } => {
                    gizmos.arrow(from, to, outline);
                }
                GizmoShape::Cone { apex, rotation, length, radius } => {
                    let color = Color::srgba(1.0, 0.9, 0.4, 0.8);
                    let base = apex + rotation * Vec3::NEG_Z * length;
                    gizmos.circle(Isometry3d::new(base, rotation), radius, color);
                    for axis in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y] {
                        gizmos.line(apex, base + rotation * axis * radius, color);
                    }
                }
            }
        }
    }
    if let Some(m) = metrics.as_deref_mut() {
        m.timings.stop(timing::GIZMOS, started);
    }
}

/// How long the speed readout stays after the last change.
const SPEED_HUD_TIME: std::time::Duration = std::time::Duration::from_millis(900);

/// The fly-speed readout: small plain text in the bottom-left corner of the viewport, only while a
/// speed is being changed with the wheel (it disappears a moment after the last notch). No box behind
/// it; a faint shadow keeps it legible on a bright sky.
pub fn paint_speed_readout(painter: &egui::Painter, ed: &Editor, rect: egui::Rect) {
    let Some((kind, until)) = ed.speed_hud else { return };
    if std::time::Instant::now() >= until || game_camera_active(ed) {
        return;
    }
    let (speed, second) = (ed.settings.camera_speed, ed.settings.fast_multiplier);
    let text = match kind {
        SpeedKind::Fly => format!("Fly Camera Speed  {speed:.1} studs/s"),
        SpeedKind::Second => format!("Secondary Camera Speed  x{second:.2}  ({:.1} studs/s)", speed * second),
    };
    let at = rect.left_bottom() + egui::vec2(14.0, -12.0);
    let font = egui::FontId::proportional(14.0);
    painter.text(at + egui::vec2(1.0, 1.0), egui::Align2::LEFT_BOTTOM, &text, font.clone(), egui::Color32::from_rgba_unmultiplied(0, 0, 0, 140));
    painter.text(at, egui::Align2::LEFT_BOTTOM, text, font, egui::Color32::from_rgba_unmultiplied(235, 235, 235, 235));
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_distance_fits_the_object_in_the_narrower_direction() {
        let fov = 70f32.to_radians();
        let wide = frame_distance(10.0, fov, 2.0);
        let narrow = frame_distance(10.0, fov, 0.5);
        assert!(narrow > wide, "a tall window needs the camera further back for the same object");
        assert!(frame_distance(20.0, fov, 1.5) > frame_distance(5.0, fov, 1.5), "bigger objects are framed from further away");
        assert_eq!(frame_distance(0.0, fov, 1.5), 3.0, "never closer than a few studs");
        assert_eq!(frame_distance(1.0e9, fov, 1.5), 5000.0);
        // The sphere really fits: at that distance its angular radius is within the field of view.
        let d = frame_distance(10.0, fov, 1.0);
        assert!((10.0f32 / d).asin() < fov * 0.5);
    }

    #[test]
    fn the_bounds_of_a_selection_cover_everything_in_it() {
        let mut dm = DataModel::new();
        let ws = dm.workspace;
        let mut make = |x: f32| {
            let p = dm.create("Part").unwrap();
            dm.set_prop(p, "Position", forge_engine::core::Value::V3(Vec3::new(x, 0.0, 0.0))).unwrap();
            dm.set_prop(p, "Size", forge_engine::core::Value::V3(Vec3::splat(2.0))).unwrap();
            dm.set_parent(p, Some(ws)).unwrap();
            p
        };
        let (a, b) = (make(-10.0), make(10.0));
        let (center, radius) = bounds_of(&dm, &[a, b]).unwrap();
        assert!(center.length() < 1e-4);
        assert!(radius > 11.0, "reaches the far corners, got {radius}");
        assert!(bounds_of(&dm, &[]).is_none());
    }

    // ---- the camera system, run for real in a headless world ----------------------------------------------------

    use std::time::Duration;

    use bevy::ecs::system::RunSystemOnce;
    use forge_engine::project::Project;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_viewport_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Viewport Test").expect("create project");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    /// A world with everything `camera_controls` reads, the mouse in the middle of the viewport.
    fn world(project: &Scratch) -> World {
        let mut ed = Editor::new(Some(project.0.clone()));
        ed.commit_edits(false);
        let mut w = World::new();
        w.insert_non_send(ed);
        w.insert_resource(Time::<()>::default());
        w.insert_resource(ButtonInput::<KeyCode>::default());
        w.insert_resource(ButtonInput::<MouseButton>::default());
        w.insert_resource(AccumulatedMouseMotion::default());
        w.insert_resource(AccumulatedMouseScroll::default());
        w.insert_resource(ViewportRect { min_pt: Vec2::new(250.0, 60.0), max_pt: Vec2::new(1250.0, 860.0), pointer: Some(Vec2::new(750.0, 460.0)), hovered: true, valid: true, ..Default::default() });
        w.insert_resource(GizmoState::default());
        w.insert_resource(EditorCam::default());
        w.spawn((Transform::default(), EditorCamera));
        w
    }

    /// One frame of `ms` milliseconds.
    fn step(w: &mut World, ms: u64) {
        w.resource_mut::<Time>().advance_by(Duration::from_millis(ms));
        w.run_system_once(camera_controls).expect("the camera system runs");
        w.resource_mut::<ButtonInput<MouseButton>>().clear();
        w.resource_mut::<ButtonInput<KeyCode>>().clear();
    }

    fn cam(w: &World) -> &EditorCam {
        w.resource::<EditorCam>()
    }

    fn find(w: &World, name: &str) -> Id {
        let ed = w.non_send::<Editor>();
        let dm = ed.dm.borrow();
        dm.find_first_child(dm.workspace, name, true).unwrap_or_else(|| panic!("no '{name}'"))
    }

    #[test]
    fn focus_frames_the_selection_and_then_the_camera_orbits_it() {
        let s = Scratch::new();
        let mut w = world(&s);
        let wall = find(&w, "Wall");
        let (center, radius) = {
            let ed = w.non_send::<Editor>();
            bounds_of(&ed.dm.borrow(), &[wall]).unwrap()
        };
        {
            let mut ed = w.non_send_mut::<Editor>();
            ed.select_only(Some(wall));
            ed.focus_request = true;
        }
        for _ in 0..30 {
            step(&mut w, 16);
        }
        let dist = (cam(&w).pos - center).length();
        let aspect = 1000.0 / 800.0;
        assert!((dist - frame_distance(radius, cam(&w).fov, aspect)).abs() < 0.05, "framed from the right distance: {dist}");
        let o = cam(&w).orbit.expect("focusing starts orbit mode");
        assert!((o.pivot - center).length() < 1e-3);
        let looking = (center - cam(&w).pos).normalize();
        assert!(looking.dot(cam(&w).rotation() * Vec3::NEG_Z) > 0.999, "the object is dead ahead");

        // Dragging with the right button now circles the object: the distance stays, the camera turns to keep it in view.
        w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(150.0, -40.0);
        let before = cam(&w).pos;
        for _ in 0..5 {
            step(&mut w, 16);
        }
        let after = cam(&w).pos;
        assert!((after - before).length() > 1.0, "the camera moved round");
        assert!(((after - center).length() - dist).abs() < 1e-3, "at a constant distance");
        assert!((center - after).normalize().dot(cam(&w).rotation() * Vec3::NEG_Z) > 0.999, "still looking at it");
        assert!(cam(&w).captured, "the mouse is captured during the drag");
        w.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
        w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::ZERO;
        step(&mut w, 16);
        assert!(!cam(&w).captured, "and let go afterwards");
        assert!(cam(&w).orbit.is_some(), "orbit mode lasts until the camera is flown by hand");
    }

    #[test]
    fn looking_around_hides_the_pointer_and_holds_it_where_it_was_without_ever_sending_it_to_the_middle() {
        let s = Scratch::new();
        let mut w = world(&s);
        let (win, middle) = (Vec2::new(1600.0, 900.0), Vec2::new(750.0, 460.0));
        let start = Vec2::new(420.0, 610.0);
        assert!(start.distance(middle) > 100.0, "the pointer starts well away from the middle of the viewport");
        let window = {
            let mut window = Window { resolution: bevy::window::WindowResolution::new(win.x as u32, win.y as u32), ..Default::default() };
            window.set_cursor_position(Some(start));
            w.spawn((window, PrimaryWindow)).id()
        };
        let cursor = |w: &World| w.entity(window).get::<Window>().unwrap().cursor_position().unwrap();
        let options = |w: &World| {
            let o = w.entity(window).get::<CursorOptions>().unwrap();
            (o.visible, o.grab_mode)
        };
        // (One schedule, run again and again: the system remembers where the pointer was across frames, which a
        // one-shot run would forget.)
        let mut schedule = Schedule::default();
        schedule.add_systems(viewport_cursor);
        let mut run = |w: &mut World| schedule.run(w);

        run(&mut w);
        assert_eq!((cursor(&w), options(&w).0), (start, true), "nothing is captured: nothing changes");

        // The right button goes down: hidden, and not moved at all.
        w.resource_mut::<EditorCam>().captured = true;
        run(&mut w);
        assert_eq!(cursor(&w), start, "no jump when the look begins");
        assert_eq!(options(&w), (false, CursorGrabMode::Locked));
        // The mouse moves a lot during the drag (the operating system moves the pointer): it is put back, never to the middle.
        for step in 1..=20 {
            let drifted = start + Vec2::new(40.0 * step as f32, -25.0 * step as f32);
            w.entity_mut(window).get_mut::<Window>().unwrap().set_cursor_position(Some(drifted));
            run(&mut w);
            assert_eq!(cursor(&w), start, "held in place however far the mouse travels (step {step})");
        }

        // Released: shown again, at the same place.
        w.resource_mut::<EditorCam>().captured = false;
        w.entity_mut(window).get_mut::<Window>().unwrap().set_cursor_position(Some(start + Vec2::new(5.0, 3.0)));
        run(&mut w);
        assert_eq!(cursor(&w), start);
        assert_eq!(options(&w), (true, CursorGrabMode::None));
        // And a second drag starts from wherever the pointer is then.
        let elsewhere = Vec2::new(1000.0, 300.0);
        w.entity_mut(window).get_mut::<Window>().unwrap().set_cursor_position(Some(elsewhere));
        w.resource_mut::<EditorCam>().captured = true;
        run(&mut w);
        w.entity_mut(window).get_mut::<Window>().unwrap().set_cursor_position(Some(elsewhere + Vec2::new(60.0, 60.0)));
        run(&mut w);
        assert_eq!(cursor(&w), elsewhere);
    }

    #[test]
    fn a_look_turns_the_camera_by_the_mouses_own_movement_whatever_the_size_of_the_viewport() {
        let s = Scratch::new();
        // A normal viewport, a tiny one, and a huge one: the same movement of the mouse turns the camera the same amount.
        let mut turned = Vec::new();
        for (min, max) in [(Vec2::new(250.0, 60.0), Vec2::new(1250.0, 860.0)), (Vec2::new(0.0, 0.0), Vec2::new(320.0, 240.0)), (Vec2::new(1200.0, 700.0), Vec2::new(5000.0, 3000.0))] {
            let mut w = world(&s);
            {
                let mut vp = w.resource_mut::<ViewportRect>();
                vp.min_pt = min;
                vp.max_pt = max;
                vp.pointer = Some((min + max) * 0.5);
            }
            let start = cam(&w).yaw;
            w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
            w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(300.0, 0.0);
            step(&mut w, 16);
            assert!(cam(&w).captured, "the look begins in a viewport of any size");
            // Far more movement than the viewport is wide: it keeps turning (nothing stops it at an edge).
            for _ in 0..30 {
                step(&mut w, 16);
            }
            turned.push(start - cam(&w).yaw);
        }
        assert!(turned.iter().all(|t| (t - turned[0]).abs() < 1e-4), "{turned:?}");
        assert!((turned[0] - 31.0 * 300.0 * 0.003).abs() < 1e-3, "300 pixels of mouse movement is 0.9 radians, 31 times: {}", turned[0]);
    }

    #[test]
    fn a_look_starts_only_over_the_viewport_and_goes_on_when_the_pointer_wanders_over_a_panel() {
        let s = Scratch::new();
        let mut w = world(&s);
        // Pressed over a panel (the viewport is not hovered): selecting and panels are not disturbed, no look starts.
        w.resource_mut::<ViewportRect>().hovered = false;
        w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(80.0, 0.0);
        let before = cam(&w).yaw;
        step(&mut w, 16);
        assert!(!cam(&w).captured && cam(&w).is_idle() && cam(&w).yaw == before, "a right-click on a panel does not turn the camera");
        w.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
        step(&mut w, 16);

        // Pressed over the viewport: the look starts, and carries on even if the pointer is then over a panel.
        w.resource_mut::<ViewportRect>().hovered = true;
        w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        step(&mut w, 16);
        assert!(cam(&w).captured);
        w.resource_mut::<ViewportRect>().hovered = false;
        w.resource_mut::<ButtonInput<MouseButton>>().clear();
        let mid = cam(&w).yaw;
        step(&mut w, 16);
        assert!(cam(&w).captured && cam(&w).yaw < mid, "the look continues until the button is released");

        // Released: normal interaction again, and a later press over a panel starts nothing.
        w.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
        step(&mut w, 16);
        assert!(!cam(&w).captured && cam(&w).is_idle());
    }

    #[test]
    fn losing_the_window_focus_ends_a_look_even_with_the_button_still_down() {
        let s = Scratch::new();
        let mut w = world(&s);
        w.spawn((Window { focused: true, ..Default::default() }, PrimaryWindow));
        w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        step(&mut w, 16);
        assert!(cam(&w).captured, "the drag begins");
        w.resource_mut::<ButtonInput<MouseButton>>().clear();
        let mut q = w.query_filtered::<&mut Window, With<PrimaryWindow>>();
        q.single_mut(&mut w).unwrap().focused = false;
        step(&mut w, 16);
        assert!(!cam(&w).captured && cam(&w).is_idle(), "the pointer is given back when the window loses focus");
    }

    #[test]
    fn flying_by_hand_or_selecting_something_else_leaves_orbit_mode() {
        let s = Scratch::new();
        let mut w = world(&s);
        let (wall, step1) = (find(&w, "Wall"), find(&w, "Step 1"));
        {
            let mut ed = w.non_send_mut::<Editor>();
            ed.select_only(Some(wall));
            ed.focus_request = true;
        }
        for _ in 0..20 {
            step(&mut w, 16);
        }
        assert!(cam(&w).orbit.is_some());
        w.non_send_mut::<Editor>().select_only(Some(step1));
        step(&mut w, 16);
        assert!(cam(&w).orbit.is_none(), "another object: the old orbit no longer applies");

        w.non_send_mut::<Editor>().focus_request = true;
        for _ in 0..20 {
            step(&mut w, 16);
        }
        assert!(cam(&w).orbit.is_some());
        w.non_send_mut::<Editor>().keys.held.insert(Action::CamForward);
        step(&mut w, 16);
        assert!(cam(&w).orbit.is_none(), "flying with the move keys leaves orbit mode");
    }

    #[test]
    fn the_move_keys_fly_the_camera_without_any_mouse_button() {
        let s = Scratch::new();
        let mut w = world(&s);
        let start = cam(&w).pos;
        let forward = cam(&w).rotation() * Vec3::NEG_Z;
        w.non_send_mut::<Editor>().keys.held.insert(Action::CamForward);
        step(&mut w, 100);
        let moved = cam(&w).pos - start;
        assert!(moved.length() > 3.5 && moved.normalize().dot(forward) > 0.999, "moved forward at the fly speed, got {moved:?}");
        assert!((cam(&w).yaw, cam(&w).pitch) == (0.0, -0.3), "no right button: the view does not turn");

        // The second speed multiplies the movement, and may be below one.
        let before = cam(&w).pos;
        {
            let mut ed = w.non_send_mut::<Editor>();
            ed.keys.held.insert(Action::FastCamera);
            ed.settings.fast_multiplier = 0.25;
        }
        step(&mut w, 100);
        let slow = (cam(&w).pos - before).length();
        assert!((slow - moved.length() * 0.25).abs() < 0.05, "a quarter of the speed: {slow} vs {}", moved.length());
    }

    #[test]
    fn alt_and_right_mouse_orbit_the_point_under_the_mouse_with_a_stable_pivot() {
        let s = Scratch::new();
        let mut w = world(&s);
        // The ground point under the middle of the viewport.
        let view = cam(&w).view(w.resource::<ViewportRect>());
        let (o, d) = view.ray(Vec2::new(750.0, 460.0)).unwrap();
        let hit = {
            let ed = w.non_send::<Editor>();
            let (_, t) = pick_collider(&ed.dm.borrow(), o, d).expect("the baseplate is under the mouse");
            o + d * t
        };
        let dist = (cam(&w).pos - hit).length();

        w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::AltLeft);
        w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(120.0, 30.0);
        for i in 0..6 {
            step(&mut w, 16);
            // The mouse is captured and recentred, so what is under the pointer changes; the pivot must not.
            assert!(((cam(&w).pos - hit).length() - dist).abs() < 1e-3, "frame {i}: distance to the pivot stays {dist}");
        }
        assert!(cam(&w).captured);
        assert!((cam(&w).yaw - 0.0).abs() > 0.1, "the view turned");
        assert!(cam(&w).orbit.is_none(), "an Alt orbit is for the length of the drag only");
        // The point stays under the pointer: it is still where the mouse is on the screen.
        let view = cam(&w).view(w.resource::<ViewportRect>());
        let p = view.project(hit).expect("still in front of the camera");
        assert!((p - Vec2::new(750.0, 460.0)).length() < 1.0, "the pivot stays under the pointer: {p:?}");
    }

    #[test]
    fn alt_orbit_ignores_the_selection_and_circles_the_point_under_the_mouse() {
        let s = Scratch::new();
        let mut w = world(&s);
        let step3 = find(&w, "Step 3");
        let selected = {
            let ed = w.non_send::<Editor>();
            bounds_of(&ed.dm.borrow(), &[step3]).unwrap().0
        };
        w.non_send_mut::<Editor>().select_only(Some(step3));
        // Whatever is under the mouse (the mouse is at the far left of the picture, so not the middle of it).
        w.resource_mut::<ViewportRect>().pointer = Some(Vec2::new(420.0, 520.0));
        let view = cam(&w).view(w.resource::<ViewportRect>());
        let (o, d) = view.ray(Vec2::new(420.0, 520.0)).unwrap();
        let hit = {
            let ed = w.non_send::<Editor>();
            let (_, t) = pick_collider(&ed.dm.borrow(), o, d).expect("the baseplate is under the mouse");
            o + d * t
        };
        let (to_hit, to_selected) = ((cam(&w).pos - hit).length(), (cam(&w).pos - selected).length());
        assert!((to_hit - to_selected).abs() > 1.0, "the two candidates are different points");
        let (pos0, yaw0, pitch0) = (cam(&w).pos, cam(&w).yaw, cam(&w).pitch);
        w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::AltLeft);
        w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(90.0, 20.0);
        // The first frame of the drag does not move the camera by itself: only the mouse's own motion turns it.
        step(&mut w, 16);
        let first = (cam(&w).pos - pos0).length();
        let turned = (90.0f32 + 20.0) * 0.003;
        assert!(first <= to_hit * turned * 1.05, "the first frame only swings round the pivot, it does not jump toward anything: {first}");
        assert!(((cam(&w).pos - hit).length() - to_hit).abs() < 1e-3);
        for i in 0..5 {
            step(&mut w, 16);
            assert!(((cam(&w).pos - hit).length() - to_hit).abs() < 1e-3, "frame {i}: circling the point under the mouse");
        }
        assert!(((cam(&w).pos - selected).length() - to_selected).abs() > 0.5, "and not the selected object");
        assert!((cam(&w).yaw - yaw0).abs() > 0.1 && (cam(&w).pitch - pitch0).abs() > 0.01, "the view turned");
        // Continuous: every frame moved the camera by about the same small amount, never a leap.
        let mut last = cam(&w).pos;
        for _ in 0..4 {
            step(&mut w, 16);
            let hop = (cam(&w).pos - last).length();
            assert!(hop <= to_hit * turned * 1.05, "smooth: {hop}");
            last = cam(&w).pos;
        }
    }

    #[test]
    fn alt_orbit_is_the_same_with_nothing_selected_and_f_focus_stays_separate() {
        let drag = |select: bool| {
            let s = Scratch::new();
            let mut w = world(&s);
            if select {
                let wall = find(&w, "Wall");
                w.non_send_mut::<Editor>().select_only(Some(wall));
            }
            w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::AltLeft);
            w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
            w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(70.0, 10.0);
            for _ in 0..4 {
                step(&mut w, 16);
            }
            (cam(&w).pos, cam(&w).yaw, cam(&w).pitch)
        };
        let (with, without) = (drag(true), drag(false));
        assert!((with.0 - without.0).length() < 1e-4 && with.1 == without.1 && with.2 == without.2, "the selection changes nothing: {with:?} vs {without:?}");
    }

    #[test]
    fn alt_orbit_with_nothing_to_hit_still_has_a_pivot_ahead() {
        let s = Scratch::new();
        let mut w = world(&s);
        // Look at the sky: nothing to hit, so the pivot is a point along the view ray.
        {
            let mut c = w.resource_mut::<EditorCam>();
            c.pitch = 0.8;
        }
        w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::AltLeft);
        w.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        w.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(60.0, 0.0);
        let before = cam(&w).pos;
        for _ in 0..3 {
            step(&mut w, 16);
        }
        let d = (cam(&w).pos - before).length();
        assert!(d > 0.5 && d.is_finite(), "orbited a pivot 30 studs ahead, moved {d}");
    }

    #[test]
    fn the_wheel_changes_the_main_and_the_second_speed_and_the_second_may_drop_below_one() {
        let s = Scratch::new();
        let mut w = world(&s);
        let wheel = |w: &mut World, action, notches: f32| {
            w.non_send_mut::<Editor>().keys.wheel = Some(action);
            w.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, notches);
            step(w, 16);
            w.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::ZERO;
            w.non_send_mut::<Editor>().keys.wheel = None;
        };
        let speed = |w: &World| w.non_send::<Editor>().settings.camera_speed;
        let second = |w: &World| w.non_send::<Editor>().settings.fast_multiplier;

        let (s0, m0) = (speed(&w), second(&w));
        wheel(&mut w, Action::PrimarySpeedWheel, 1.0);
        assert!((speed(&w) - s0 * 1.1).abs() < 1e-3, "one notch up is +10 %");
        wheel(&mut w, Action::PrimarySpeedWheel, -3.0);
        assert!(speed(&w) < s0);
        for _ in 0..200 {
            wheel(&mut w, Action::PrimarySpeedWheel, -1.0);
        }
        assert_eq!(speed(&w), MIN_FLY_SPEED, "never to zero");
        assert_eq!(second(&w), m0, "the second speed was not touched");

        for _ in 0..60 {
            wheel(&mut w, Action::FastSpeedWheel, -1.0);
        }
        assert!(second(&w) < 1.0, "the second speed can go below 1x, got {}", second(&w));
        assert!(second(&w) >= MIN_SECOND_SPEED);
        assert_eq!(w.non_send::<Editor>().speed_hud.map(|(k, _)| k), Some(SpeedKind::Second), "the new value is shown on screen");
    }

    /// Turns the wheel `notches` for one frame (with the zoom shortcut), then lets the camera settle.
    fn zoom(w: &mut World, notches: f32) {
        w.non_send_mut::<Editor>().keys.wheel = Some(Action::ZoomWheel);
        w.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, notches);
        step(w, 16);
        w.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::ZERO;
    }

    fn settle(w: &mut World) {
        for _ in 0..60 {
            step(w, 16);
        }
    }

    #[test]
    fn zooming_goes_along_the_view_whatever_is_selected() {
        let travel = |select: Option<&str>| {
            let s = Scratch::new();
            let mut w = world(&s);
            w.resource_mut::<EditorCam>().yaw = 0.7; // looking away from the objects at the origin
            if let Some(name) = select {
                let id = find(&w, name);
                w.non_send_mut::<Editor>().select_only(Some(id));
            }
            let start = cam(&w).pos;
            zoom(&mut w, 2.0);
            settle(&mut w);
            let forward = cam(&w).rotation() * Vec3::NEG_Z;
            (cam(&w).pos - start, forward)
        };
        let (none, forward) = travel(None);
        assert!(none.length() > 1.0 && none.normalize().dot(forward) > 0.9999, "straight along the view: {none:?}");
        for name in ["Wall", "Step 3"] {
            let (with, _) = travel(Some(name));
            assert!((with - none).length() < 1e-3, "selecting {name} changes nothing: {with:?} vs {none:?}");
        }
        // And back out along the same line.
        let s = Scratch::new();
        let mut w = world(&s);
        let start = cam(&w).pos;
        zoom(&mut w, 3.0);
        settle(&mut w);
        zoom(&mut w, -3.0);
        settle(&mut w);
        assert!((cam(&w).pos - start).length() < 0.05, "in and out comes back: {:?}", cam(&w).pos - start);
    }

    #[test]
    fn selecting_while_the_zoom_is_still_travelling_does_not_bend_it() {
        let s = Scratch::new();
        let mut w = world(&s);
        let wall = find(&w, "Wall");
        let start = cam(&w).pos;
        let forward = cam(&w).rotation() * Vec3::NEG_Z;
        zoom(&mut w, 4.0);
        w.non_send_mut::<Editor>().select_only(Some(wall));
        step(&mut w, 16);
        settle(&mut w);
        assert!((cam(&w).pos - start).normalize().dot(forward) > 0.9999);
    }

    #[test]
    fn hover_based_zoom_goes_toward_the_pointer_and_camera_based_zoom_ignores_it() {
        let go = |mode: ZoomMode, pointer: Vec2| {
            let s = Scratch::new();
            let mut w = world(&s);
            w.non_send_mut::<Editor>().settings.zoom_mode = mode;
            w.resource_mut::<ViewportRect>().pointer = Some(pointer);
            let start = cam(&w).pos;
            let view = cam(&w).view(w.resource::<ViewportRect>());
            let ray = view.ray(pointer).unwrap().1;
            zoom(&mut w, 2.0);
            settle(&mut w);
            let forward = cam(&w).rotation() * Vec3::NEG_Z;
            ((cam(&w).pos - start).normalize(), ray, forward)
        };
        let off_centre = Vec2::new(380.0, 300.0);
        let (dir, ray, forward) = go(ZoomMode::Hover, off_centre);
        assert!(dir.dot(ray) > 0.9999 && dir.dot(forward) < 0.999, "toward the pointer: {dir:?}");
        let (dir, _, forward) = go(ZoomMode::Camera, off_centre);
        assert!(dir.dot(forward) > 0.9999, "camera-based ignores the pointer: {dir:?}");

        // Hover-based with a selection does not turn into focusing the selection, and with the pointer nowhere it
        // falls back to the view.
        let s = Scratch::new();
        let mut w = world(&s);
        let wall = find(&w, "Wall");
        {
            let mut ed = w.non_send_mut::<Editor>();
            ed.settings.zoom_mode = ZoomMode::Hover;
            ed.select_only(Some(wall));
        }
        w.resource_mut::<ViewportRect>().pointer = None;
        let start = cam(&w).pos;
        zoom(&mut w, 2.0);
        settle(&mut w);
        let forward = cam(&w).rotation() * Vec3::NEG_Z;
        assert!((cam(&w).pos - start).normalize().dot(forward) > 0.9999, "no pointer: along the view");
    }

    #[test]
    fn a_notch_is_a_small_eased_move_that_stops_short_of_a_surface() {
        let s = Scratch::new();
        let mut w = world(&s);
        let start = cam(&w).pos;
        zoom(&mut w, 1.0);
        let first = (cam(&w).pos - start).length();
        let mut moves = vec![first];
        let mut last = cam(&w).pos;
        for _ in 0..40 {
            step(&mut w, 16);
            moves.push((cam(&w).pos - last).length());
            last = cam(&w).pos;
        }
        let total: f32 = moves.iter().sum();
        assert!(total > 0.5 && total < 10.0, "one notch is a modest step, not a leap: {total}");
        assert!(first < total * 0.5, "it eases in over several frames instead of jumping: {first} of {total}");
        let visible: Vec<f32> = moves.iter().copied().filter(|m| *m > 2e-3).collect();
        assert!(visible.windows(2).all(|m| m[1] <= m[0] + 1e-4), "and slows steadily to a stop: {visible:?}");
        assert_eq!(cam(&w).zoom_remaining(), 0.0);

        // Close to the ground, looking down: the steps get small and the camera never goes through.
        let mut w = world(&Scratch::new());
        {
            let mut c = w.resource_mut::<EditorCam>();
            c.pos = Vec3::new(0.0, 3.0, 0.0);
            c.pitch = -1.5;
            c.yaw = 0.0;
        }
        let ground = {
            let ed = w.non_send::<Editor>();
            let c = cam(&w);
            let (_, t) = pick_collider(&ed.dm.borrow(), c.pos, c.rotation() * Vec3::NEG_Z).expect("ground below");
            t
        };
        let before = cam(&w).pos;
        zoom(&mut w, 1.0);
        settle(&mut w);
        assert!((cam(&w).pos - before).length() <= ground * 0.12, "near a surface a notch is a small step");
        for _ in 0..30 {
            zoom(&mut w, 3.0);
        }
        settle(&mut w);
        let left = {
            let ed = w.non_send::<Editor>();
            let c = cam(&w);
            pick_collider(&ed.dm.borrow(), c.pos, c.rotation() * Vec3::NEG_Z).map(|(_, t)| t)
        };
        assert!(left.is_some_and(|t| t >= ZOOM_STOP_DISTANCE - 1e-3), "zooming in stops {ZOOM_STOP_DISTANCE} from the surface: {left:?}");
        // Zooming out from there works and speeds up with distance.
        let mut last = cam(&w).pos;
        let mut hops = Vec::new();
        for _ in 0..4 {
            zoom(&mut w, -1.0);
            settle(&mut w);
            hops.push((cam(&w).pos - last).length());
            last = cam(&w).pos;
        }
        assert!(hops.windows(2).all(|h| h[1] >= h[0] - 1e-3) && hops[3] > hops[0], "steps grow as the surface gets further: {hops:?}");
    }

    #[test]
    fn after_focus_the_wheel_zooms_around_the_focus_point() {
        let s = Scratch::new();
        let mut w = world(&s);
        let wall = find(&w, "Wall");
        let center = {
            let ed = w.non_send::<Editor>();
            bounds_of(&ed.dm.borrow(), &[wall]).unwrap().0
        };
        {
            let mut ed = w.non_send_mut::<Editor>();
            ed.select_only(Some(wall));
            ed.focus_request = true;
        }
        for _ in 0..30 {
            step(&mut w, 16);
        }
        let before = (cam(&w).pos - center).length();
        zoom(&mut w, 2.0);
        settle(&mut w);
        let after = (cam(&w).pos - center).length();
        assert!(after < before - 0.5, "closer to the focus point: {before} -> {after}");
        let looking = (center - cam(&w).pos).normalize();
        assert!(looking.dot(cam(&w).rotation() * Vec3::NEG_Z) > 0.999, "still looking at it");
        assert!((cam(&w).orbit.unwrap().dist - after).abs() < 1e-2);
        // Only the Focus request frames the selection: a plain zoom after selecting something else never does.
        let step1 = find(&w, "Step 1");
        w.non_send_mut::<Editor>().select_only(Some(step1));
        step(&mut w, 16);
        let pos = cam(&w).pos;
        step(&mut w, 16);
        assert_eq!(cam(&w).pos, pos, "selecting moves nothing");
    }

    #[test]
    fn the_speed_wheels_keep_working_when_the_pointer_has_left_the_viewport_while_flying_but_zoom_does_not() {
        let s = Scratch::new();
        let mut w = world(&s);
        w.resource_mut::<ViewportRect>().hovered = false;
        w.non_send_mut::<Editor>().nav_latched = true;
        let speed = |w: &World| w.non_send::<Editor>().settings.camera_speed;
        let s0 = speed(&w);
        w.non_send_mut::<Editor>().keys.wheel = Some(Action::PrimarySpeedWheel);
        w.resource_mut::<AccumulatedMouseScroll>().delta = Vec2::new(0.0, 1.0);
        step(&mut w, 16);
        assert!(speed(&w) > s0, "adjusting the fly speed while flying");
        let start = cam(&w).pos;
        w.non_send_mut::<Editor>().keys.wheel = Some(Action::ZoomWheel);
        step(&mut w, 16);
        settle(&mut w);
        assert_eq!(cam(&w).pos, start, "zooming needs the pointer over the picture");
    }

    // ---- picking and the tools, through the real system ---------------------------------------------------------

    fn point_at(w: &mut World, world_pos: Vec3) {
        let view = cam(w).view(w.resource::<ViewportRect>());
        let p = view.project(world_pos).expect("in front of the camera");
        w.resource_mut::<ViewportRect>().pointer = Some(p);
    }

    fn mouse(w: &mut World, shift: bool, alt: bool, press: bool) {
        {
            let mut keys = w.resource_mut::<ButtonInput<KeyCode>>();
            for (code, on) in [(KeyCode::ShiftLeft, shift), (KeyCode::AltLeft, alt)] {
                if on {
                    keys.press(code);
                } else {
                    keys.release(code);
                }
            }
        }
        let mut buttons = w.resource_mut::<ButtonInput<MouseButton>>();
        if press {
            buttons.press(MouseButton::Left);
        } else {
            buttons.release(MouseButton::Left);
        }
        drop(buttons);
        w.run_system_once(pick_and_move).expect("the picking system runs");
        w.resource_mut::<ButtonInput<MouseButton>>().clear();
    }

    /// A click (press, then release) with the given modifiers.
    fn click(w: &mut World, shift: bool, alt: bool) {
        mouse(w, shift, alt, true);
        mouse(w, shift, alt, false);
    }

    fn ed(w: &World) -> &Editor {
        w.non_send::<Editor>()
    }

    #[test]
    fn clicking_selects_shift_adds_and_clicking_empty_space_clears() {
        let s = Scratch::new();
        let mut w = world(&s);
        let (wall, step1) = (find(&w, "Wall"), find(&w, "Step 1"));
        let aim = |w: &mut World, id: Id| {
            let (c, _) = bounds_of(&w.non_send::<Editor>().dm.borrow(), &[id]).unwrap();
            point_at(w, c);
        };
        // (Looking from the default camera position the template's wall and first step are both in view.)
        w.resource_mut::<EditorCam>().pos = Vec3::new(-10.0, 12.0, 40.0);
        aim(&mut w, wall);
        click(&mut w, false, false);
        assert_eq!(ed(&w).selection, vec![wall]);
        aim(&mut w, step1);
        click(&mut w, true, false);
        assert_eq!(ed(&w).selection, vec![wall, step1], "shift adds");
        click(&mut w, true, false);
        assert_eq!(ed(&w).selection, vec![wall], "shift again takes it out");
        w.resource_mut::<ViewportRect>().pointer = Some(Vec2::new(260.0, 70.0));
        w.resource_mut::<EditorCam>().pitch = 0.9; // looking at the sky
        click(&mut w, false, false);
        assert!(ed(&w).selection.is_empty(), "empty space clears the selection");
    }

    #[test]
    fn alt_clicks_cycle_through_stacked_objects_and_shift_alt_collects_them() {
        let s = Scratch::new();
        let mut w = world(&s);
        // Three parts one behind another, straight ahead of the camera.
        let stack: Vec<Id> = {
            let ed = w.non_send::<Editor>();
            let mut dm = ed.dm.borrow_mut();
            let ws = dm.workspace;
            (0..3)
                .map(|i| {
                    let p = dm.create("Part").unwrap();
                    dm.set_prop(p, "Size", forge_engine::core::Value::V3(Vec3::splat(4.0))).unwrap();
                    dm.set_prop(p, "Position", forge_engine::core::Value::V3(Vec3::new(0.0, 14.0, 10.0 - i as f32 * 8.0))).unwrap();
                    dm.set_parent(p, Some(ws)).unwrap();
                    p
                })
                .collect()
        };
        {
            let mut c = w.resource_mut::<EditorCam>();
            c.pos = Vec3::new(0.0, 14.0, 40.0);
            c.pitch = 0.0;
            c.yaw = 0.0;
        }
        point_at(&mut w, Vec3::new(0.0, 14.0, 0.0));
        click(&mut w, false, true);
        assert_eq!(ed(&w).selected, Some(stack[0]), "the nearest first");
        click(&mut w, false, true);
        assert_eq!(ed(&w).selected, Some(stack[1]), "then the one behind it");
        click(&mut w, false, true);
        assert_eq!(ed(&w).selected, Some(stack[2]));
        click(&mut w, false, true);
        assert_eq!(ed(&w).selected, Some(stack[0]), "and round again");
        assert_eq!(ed(&w).selection, vec![stack[0]], "Alt alone replaces the selection");

        // Shift + Alt: the same walk, keeping what was already selected.
        w.non_send_mut::<Editor>().select_only(None);
        w.non_send_mut::<Editor>().pick_cycle.reset();
        click(&mut w, true, true);
        click(&mut w, true, true);
        click(&mut w, true, true);
        assert_eq!(ed(&w).selection, vec![stack[0], stack[1], stack[2]]);
    }

    #[test]
    fn clicking_a_part_of_a_group_selects_the_group_and_alt_selects_the_part() {
        let s = Scratch::new();
        let mut w = world(&s);
        // A Model (inside a Folder) holding a part, and a bare part, side by side in front of the camera.
        let (folder, model, inner, bare) = {
            let ed = w.non_send::<Editor>();
            let mut dm = ed.dm.borrow_mut();
            let ws = dm.workspace;
            let folder = dm.create("Folder").unwrap();
            dm.set_parent(folder, Some(ws)).unwrap();
            let model = dm.create("Model").unwrap();
            dm.set_parent(model, Some(folder)).unwrap();
            let mk = |dm: &mut DataModel, x: f32, parent: Id| {
                let p = dm.create("Part").unwrap();
                dm.set_prop(p, "Size", forge_engine::core::Value::V3(Vec3::splat(4.0))).unwrap();
                dm.set_prop(p, "Position", forge_engine::core::Value::V3(Vec3::new(x, 14.0, 0.0))).unwrap();
                dm.set_parent(p, Some(parent)).unwrap();
                p
            };
            let inner = mk(&mut dm, -4.0, model);
            let bare = mk(&mut dm, 6.0, ws);
            (folder, model, inner, bare)
        };
        {
            let mut c = w.resource_mut::<EditorCam>();
            c.pos = Vec3::new(0.0, 14.0, 40.0);
            c.pitch = 0.0;
            c.yaw = 0.0;
        }
        point_at(&mut w, Vec3::new(-4.0, 14.0, 0.0));
        click(&mut w, false, false);
        assert_eq!(ed(&w).selection, vec![model], "a part of a group selects the group, not the folder or the part");
        click(&mut w, false, true);
        assert_eq!(ed(&w).selection, vec![inner], "Alt selects the part under the mouse, inside the group");
        click(&mut w, false, false);
        assert_eq!(ed(&w).selection, vec![model], "and a plain click is the group again");
        assert_ne!(model, folder);

        point_at(&mut w, Vec3::new(6.0, 14.0, 0.0));
        click(&mut w, false, false);
        assert_eq!(ed(&w).selection, vec![bare], "a part that is in no group is selected itself");
        // Shift adds the group as a whole, too.
        point_at(&mut w, Vec3::new(-4.0, 14.0, 0.0));
        click(&mut w, true, false);
        assert_eq!(ed(&w).selection, vec![bare, model]);
    }

    #[test]
    fn dragging_a_move_arrow_with_the_mouse_moves_the_selection_in_one_undo_step() {
        let s = Scratch::new();
        let mut w = world(&s);
        let wall = find(&w, "Wall");
        w.resource_mut::<EditorCam>().pos = Vec3::new(-10.0, 20.0, 60.0);
        let start = ed(&w).dm.borrow().vec3(wall, "Position");
        {
            let mut e = w.non_send_mut::<Editor>();
            e.select_only(Some(wall));
            e.tool = crate::state::Tool::Move;
            e.settings.move_increment = 1.0;
        }
        // Grab the +X arrow half way along.
        let frame = tools::active_frame(ed(&w), cam(&w), None).unwrap();
        let grab = frame.origin + Vec3::X * (frame.len * 0.5);
        point_at(&mut w, grab);
        // Hover first (the arrow lights up), then press, then drag.
        w.run_system_once(pick_and_move).unwrap();
        assert!(w.resource::<GizmoState>().hover.is_some(), "the arrow lights up under the mouse");
        mouse(&mut w, false, false, true);
        assert!(w.resource::<GizmoState>().dragging(), "pressing on it takes hold");
        assert!(ed(&w).selection == vec![wall], "grabbing a handle does not change the selection");
        point_at(&mut w, grab + Vec3::X * 6.2);
        w.run_system_once(pick_and_move).unwrap();
        w.non_send_mut::<Editor>().commit_edits(true); // still dragging: nothing committed yet
        assert!(!ed(&w).history.can_undo());
        let moved = ed(&w).dm.borrow().vec3(wall, "Position");
        assert!((moved.x - start.x - 6.0).abs() < 1e-3, "moved 6 studs (snapped), got {}", moved.x - start.x);
        assert_eq!((moved.y, moved.z), (start.y, start.z));
        mouse(&mut w, false, false, false);
        assert!(!w.resource::<GizmoState>().dragging(), "releasing lets go");
        w.non_send_mut::<Editor>().commit_edits(false);
        w.non_send_mut::<Editor>().undo();
        assert_eq!(ed(&w).dm.borrow().vec3(wall, "Position"), start, "the whole drag is one undo step");
    }

    #[test]
    fn dragging_with_the_pivot_tool_moves_the_pivot_in_one_undo_step_and_hides_the_gizmo_while_dragging() {
        let s = Scratch::new();
        let mut w = world(&s);
        let wall = find(&w, "Wall");
        w.resource_mut::<EditorCam>().pos = Vec3::new(-10.0, 20.0, 60.0);
        let (centre, size) = {
            let e = ed(&w);
            (e.dm.borrow().vec3(wall, "Position"), e.dm.borrow().size(wall))
        };
        {
            let mut e = w.non_send_mut::<Editor>();
            e.select_only(Some(wall));
            e.tool = crate::state::Tool::Pivot;
            e.settings.move_increment = 1.0;
            e.commit_edits(false);
        }
        let frame = tools::active_frame(ed(&w), cam(&w), None).unwrap();
        assert_eq!(frame.origin, centre, "the pivot starts at the centre");
        let grab = frame.origin + Vec3::X * (frame.len * 0.5);
        point_at(&mut w, grab);
        w.run_system_once(pick_and_move).unwrap();
        mouse(&mut w, false, false, true);
        assert!(w.resource::<GizmoState>().dragging());
        point_at(&mut w, grab + Vec3::X * 5.1);
        w.run_system_once(pick_and_move).unwrap();
        mouse(&mut w, false, false, false);
        w.non_send_mut::<Editor>().commit_edits(false);

        let e = ed(&w);
        assert_eq!(e.dm.borrow().vec3(wall, "Position"), centre, "the object itself stays put");
        assert_eq!(e.dm.borrow().size(wall), size);
        assert!((e.dm.borrow().vec3(wall, "PivotOffset") - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-3);
        // The move tool's gizmo now sits at the pivot.
        w.non_send_mut::<Editor>().tool = crate::state::Tool::Move;
        assert!((tools::active_frame(ed(&w), cam(&w), None).unwrap().origin - (centre + Vec3::X * 5.0)).length() < 1e-3);
        w.non_send_mut::<Editor>().undo();
        assert_eq!(ed(&w).dm.borrow().vec3(wall, "PivotOffset"), Vec3::ZERO, "one undo puts the pivot back");
    }

    #[test]
    fn shift_or_alt_clicks_pass_through_the_handles_to_select() {
        let s = Scratch::new();
        let mut w = world(&s);
        let (wall, step1) = (find(&w, "Wall"), find(&w, "Step 1"));
        w.resource_mut::<EditorCam>().pos = Vec3::new(-10.0, 20.0, 60.0);
        {
            let mut e = w.non_send_mut::<Editor>();
            e.select_only(Some(wall));
            e.tool = crate::state::Tool::Move;
        }
        let frame = tools::active_frame(ed(&w), cam(&w), None).unwrap();
        point_at(&mut w, frame.origin + Vec3::X * (frame.len * 0.5));
        w.run_system_once(pick_and_move).unwrap();
        assert!(w.resource::<GizmoState>().hover.is_some());
        mouse(&mut w, true, false, true); // shift held
        assert!(!w.resource::<GizmoState>().dragging(), "with Shift the click selects instead of grabbing the handle");
        let _ = step1;
    
    }

    // ---- the Select tool picks objects up, and the Scale tool's modifiers --------------------------------------------

    /// Keeps these keys held (and releases the others of Shift / Ctrl / Alt).
    fn hold(w: &mut World, shift: bool, ctrl: bool, alt: bool) {
        let mut keys = w.resource_mut::<ButtonInput<KeyCode>>();
        for (code, on) in [(KeyCode::ShiftLeft, shift), (KeyCode::ControlLeft, ctrl), (KeyCode::AltLeft, alt)] {
            if on {
                keys.press(code);
            } else {
                keys.release(code);
            }
        }
    }

    /// One frame of the picking system, with the buttons as they are.
    fn frame(w: &mut World) {
        w.run_system_once(pick_and_move).expect("the picking system runs");
        w.resource_mut::<ButtonInput<MouseButton>>().clear();
    }

    fn pointer_to(w: &mut World, p: Vec2) {
        w.resource_mut::<ViewportRect>().pointer = Some(p);
    }

    fn pointer_now(w: &World) -> Vec2 {
        w.resource::<ViewportRect>().pointer.unwrap()
    }

    fn pos(w: &World, id: Id) -> Vec3 {
        ed(w).dm.borrow().vec3(id, "Position")
    }

    /// A camera that looks straight ahead along -Z from a little way back, and the wall's centre on screen.
    fn setup_select(w: &mut World) -> (Id, Id) {
        let (wall, step1) = (find(w, "Wall"), find(w, "Step 1"));
        {
            let mut c = w.resource_mut::<EditorCam>();
            c.pos = Vec3::new(-10.0, 12.0, 40.0);
        }
        (wall, step1)
    }

    fn centre_of(w: &World, id: Id) -> Vec3 {
        bounds_of(&ed(w).dm.borrow(), &[id]).unwrap().0
    }

    /// Turns the camera, from high above and well back, to look at an object: steep enough that an object picked up
    /// slides on the level through where it was grabbed (a level view uses the plane facing the camera instead).
    fn look_down_at(w: &mut World, id: Id) {
        let target = centre_of(w, id);
        let from = target + Vec3::new(-6.0, 40.0, 30.0);
        let d = (target - from).normalize();
        let mut c = w.resource_mut::<EditorCam>();
        (c.pos, c.yaw, c.pitch) = (from, (-d.x).atan2(-d.z), d.y.asin());
    }

    /// Points the mouse at the middle of an object.
    fn aim(w: &mut World, id: Id) {
        let c = centre_of(w, id);
        point_at(w, c);
    }

    #[test]
    fn pressing_an_object_with_the_select_tool_selects_it_and_moving_the_mouse_carries_it_along() {
        let s = Scratch::new();
        let mut w = world(&s);
        let (wall, _) = setup_select(&mut w);
        let start = pos(&w, wall);
        aim(&mut w, wall);
        let there = pointer_now(&w);
        mouse(&mut w, false, false, true);
        assert_eq!(ed(&w).selection, vec![wall], "the press selects it at once");
        assert!(w.resource::<GizmoState>().press.is_some() && !w.resource::<GizmoState>().dragging(), "but nothing moves yet");

        // A wobble is still a click.
        pointer_to(&mut w, there + Vec2::new(2.0, 1.0));
        frame(&mut w);
        assert_eq!(pos(&w, wall), start);
        assert!(!w.resource::<GizmoState>().dragging());

        // A real movement picks it up and sets it down under the mouse (on the ground it already stood on: same height).
        pointer_to(&mut w, there + Vec2::new(90.0, 0.0));
        frame(&mut w);
        assert!(w.resource::<GizmoState>().dragging(), "moved far enough: it is carried");
        let moved = pos(&w, wall) - start;
        assert!(moved.length() > 1.0 && moved.y.abs() < 1e-3, "{moved:?}");
        // Keeps following the mouse (further to the right on screen is further to the right in the world, here), and
        // gives it up at the release.
        let first = pos(&w, wall);
        pointer_to(&mut w, there + Vec2::new(150.0, 0.0));
        frame(&mut w);
        assert!(pos(&w, wall).x > first.x, "{:?} then {:?}", first, pos(&w, wall));
        mouse(&mut w, false, false, false);
        assert!(!w.resource::<GizmoState>().dragging());
        let end = pos(&w, wall);
        pointer_to(&mut w, there + Vec2::new(300.0, 0.0));
        frame(&mut w);
        assert_eq!(pos(&w, wall), end, "after the release the mouse leaves it alone");

        // The whole carry is one undo step.
        w.non_send_mut::<Editor>().commit_edits(false);
        w.non_send_mut::<Editor>().undo();
        assert_eq!(pos(&w, wall), start);
    }

    /// The nearest surface along a ray among everything except `except` (what a carried object is set down on): point and normal.
    fn surface_below(w: &World, except: Id, ray: (Vec3, Vec3)) -> (Vec3, Vec3) {
        let ed = ed(w);
        let dm = ed.dm.borrow();
        dm.colliders()
            .into_iter()
            .filter(|id| *id != except && dm.in_workspace(*id))
            .filter_map(|id| forge_engine::physics::ray_collider(&dm.collider(id)?, ray.0, ray.1))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(t, n)| (ray.0 + ray.1 * t, n))
            .expect("something is under the mouse")
    }

    #[test]
    fn a_carried_object_is_set_down_on_the_surface_under_the_cursor_centred_on_the_mouse() {
        let s = Scratch::new();
        let mut w = world(&s);
        let (wall, _) = setup_select(&mut w);
        w.non_send_mut::<Editor>().settings.move_increment = 0.0; // (with snapping the object lands on the grid)
        look_down_at(&mut w, wall);
        aim(&mut w, wall);
        let there = pointer_now(&w);
        let half = {
            let (min, max) = ed(&w).dm.borrow().collider(wall).unwrap().aabb();
            (max - min) * 0.5
        };
        mouse(&mut w, false, false, true);
        for step in 1..=6 {
            pointer_to(&mut w, there + Vec2::new(25.0 * step as f32, 10.0 * step as f32));
            frame(&mut w);
        }
        assert!(w.resource::<GizmoState>().dragging());
        // It rests on whatever is under the cursor, with the point the mouse is at in the middle of the side that touches ...
        let view = cam(&w).view(w.resource::<ViewportRect>());
        let (point, normal) = surface_below(&w, wall, view.ray(pointer_now(&w)).unwrap());
        let expected = point + normal * normal.abs().dot(half);
        let centre = centre_of(&w, wall);
        assert!((centre - expected).length() < 1e-2, "centre {centre:?}, set down on {point:?} it should be {expected:?}");
        // ... touching it, neither sunk in nor hovering.
        assert!(((centre - point).dot(normal) - normal.abs().dot(half)).abs() < 1e-2);
        // And it follows the mouse: another position of the cursor, another place for the object, again right under it.
        pointer_to(&mut w, there + Vec2::new(-60.0, 40.0));
        frame(&mut w);
        let view = cam(&w).view(w.resource::<ViewportRect>());
        let (point, normal) = surface_below(&w, wall, view.ray(pointer_now(&w)).unwrap());
        assert!((centre_of(&w, wall) - (point + normal * normal.abs().dot(half))).length() < 1e-2);
    }
    #[test]
    fn a_click_that_does_not_move_only_selects_and_clicking_one_of_several_narrows_the_selection_on_release() {
        let s = Scratch::new();
        let mut w = world(&s);
        let (wall, step1) = setup_select(&mut w);
        let (wall_start, step_start) = (pos(&w, wall), pos(&w, step1));
        aim(&mut w, step1);
        click(&mut w, false, false);
        aim(&mut w, wall);
        click(&mut w, true, false);
        assert_eq!(ed(&w).selection, vec![step1, wall]);

        // Pressing one of the two and clicking (no movement) leaves both until the release, then keeps just that one.
        mouse(&mut w, false, false, true);
        assert_eq!(ed(&w).selection, vec![step1, wall], "the press does not narrow the selection: it may be about to carry both");
        mouse(&mut w, false, false, false);
        assert_eq!(ed(&w).selection, vec![wall], "a click does");
        assert_eq!((pos(&w, wall), pos(&w, step1)), (wall_start, step_start), "and nothing moved");

        // Pressing one of two and dragging carries both, and keeps both selected.
        w.non_send_mut::<Editor>().select_only(Some(step1));
        aim(&mut w, wall);
        click(&mut w, true, false);
        assert_eq!(ed(&w).selection.len(), 2);
        let there = pointer_now(&w);
        mouse(&mut w, false, false, true);
        pointer_to(&mut w, there + Vec2::new(80.0, 0.0));
        frame(&mut w);
        assert!(w.resource::<GizmoState>().dragging());
        let (dw, ds) = (pos(&w, wall) - wall_start, pos(&w, step1) - step_start);
        assert!(dw.length() > 1.0 && (dw - ds).length() < 1e-3, "both moved by the same amount: {dw:?} {ds:?}");
        mouse(&mut w, false, false, false);
        assert_eq!(ed(&w).selection.len(), 2, "still both selected after the drag");
    }

    #[test]
    fn shift_and_alt_presses_select_and_never_pick_up_and_other_tools_do_not_either() {
        let s = Scratch::new();
        let mut w = world(&s);
        let (wall, _) = setup_select(&mut w);
        let start = pos(&w, wall);
        aim(&mut w, wall);
        let there = pointer_now(&w);
        for (shift, alt) in [(true, false), (false, true)] {
            mouse(&mut w, shift, alt, true);
            assert!(w.resource::<GizmoState>().press.is_none(), "shift {shift} alt {alt}: no pick-up");
            pointer_to(&mut w, there + Vec2::new(120.0, 0.0));
            frame(&mut w);
            assert_eq!(pos(&w, wall), start);
            mouse(&mut w, shift, alt, false);
            pointer_to(&mut w, there);
        }
        hold(&mut w, false, false, false);
        w.non_send_mut::<Editor>().tool = crate::state::Tool::Move;
        w.non_send_mut::<Editor>().select_only(None);
        mouse(&mut w, false, false, true);
        assert!(w.resource::<GizmoState>().press.is_none(), "only the Select tool carries objects");
    }

    /// The Scale tool on `id`, with the +X handle held and the mouse `pull` studs further along X.
    fn grab_scale_handle(w: &mut World, id: Id) -> (Vec3, Vec3) {
        {
            let mut e = w.non_send_mut::<Editor>();
            e.select_only(Some(id));
            e.tool = crate::state::Tool::Scale;
            e.settings.move_increment = 0.0;
        }
        let frame_ = tools::active_frame(ed(w), cam(w), None).unwrap();
        let handle = frame_.bounds.unwrap().handle(cam(w).pos, 0, 1.0);
        point_at(w, handle);
        frame(w);
        assert_eq!(w.resource::<GizmoState>().hover, Some(crate::tools::Handle::Scale(0, 1.0)), "the sphere lights up");
        mouse(w, false, false, true);
        assert!(w.resource::<GizmoState>().dragging());
        (handle, frame_.bounds.unwrap().face(0, 1.0))
    }

    #[test]
    fn shift_and_ctrl_change_a_scale_drag_while_it_is_going_on() {
        let s = Scratch::new();
        let mut w = world(&s);
        let (_, step1) = setup_select(&mut w);
        w.resource_mut::<EditorCam>().pos = Vec3::new(-10.0, 20.0, 60.0);
        let (size0, centre0) = (ed(&w).dm.borrow().size(step1), pos(&w, step1));
        let (handle, _) = grab_scale_handle(&mut w, step1);
        point_at(&mut w, handle + Vec3::X * 2.0);
        frame(&mut w);
        let plain = ed(&w).dm.borrow().size(step1);
        assert!((plain.x - size0.x - 2.0).abs() < 1e-2 && plain.y == size0.y && plain.z == size0.z, "plain: only X grows, got {plain:?} from {size0:?}");

        // Shift pressed in the middle of the drag: the other sizes follow at once, with no new mouse movement.
        hold(&mut w, true, false, false);
        frame(&mut w);
        let factor = plain.x / size0.x;
        let prop = ed(&w).dm.borrow().size(step1);
        assert!((prop - size0 * factor).length() < 1e-2, "proportional: {prop:?} is {size0:?} times {factor}");

        // Ctrl as well: about the centre.
        hold(&mut w, true, true, false);
        frame(&mut w);
        assert!((pos(&w, step1) - centre0).length() < 1e-3, "the centre stays put with Ctrl");
        let both = ed(&w).dm.borrow().size(step1);
        assert!((both.x - (size0.x + 4.0)).abs() < 1e-2 && (both - size0 * (both.x / size0.x)).length() < 1e-2, "both: {both:?}");

        // Released again: back to exactly the plain result, nothing left over.
        hold(&mut w, false, false, false);
        frame(&mut w);
        assert!((ed(&w).dm.borrow().size(step1) - plain).length() < 1e-4);
        mouse(&mut w, false, false, false);
        assert!(!w.resource::<GizmoState>().dragging());
    }

    #[test]
    fn alt_puts_the_scale_handles_on_one_part_of_a_model_and_scales_only_that_part() {
        let s = Scratch::new();
        let mut w = world(&s);
        {
            let mut c = w.resource_mut::<EditorCam>();
            c.pos = Vec3::new(0.0, 14.0, 40.0);
            c.yaw = 0.0;
            c.pitch = 0.0;
        }
        let (model, a, b) = {
            let e = ed(&w);
            let mut dm = e.dm.borrow_mut();
            let ws = dm.workspace;
            let model = dm.create("Model").unwrap();
            dm.set_parent(model, Some(ws)).unwrap();
            let mut part = |x: f32| {
                let p = dm.create("Part").unwrap();
                dm.set_prop(p, "Size", forge_engine::core::Value::V3(Vec3::splat(4.0))).unwrap();
                dm.set_prop(p, "Position", forge_engine::core::Value::V3(Vec3::new(x, 14.0, 0.0))).unwrap();
                dm.set_prop(p, "Anchored", forge_engine::core::Value::Bool(true)).unwrap();
                p
            };
            let (a, b) = (part(-8.0), part(8.0));
            dm.set_parent(a, Some(model)).unwrap();
            dm.set_parent(b, Some(model)).unwrap();
            (model, a, b)
        };
        {
            let mut e = w.non_send_mut::<Editor>();
            e.select_only(Some(model));
            e.tool = crate::state::Tool::Scale;
            e.settings.move_increment = 0.0;
        }
        // Without Alt the handles are on the whole model.
        let whole = tools::active_frame(ed(&w), cam(&w), None).unwrap();
        assert!(!whole.own_box && (whole.bounds.unwrap().half.x - 10.0).abs() < 1e-3);

        // With Alt held and the mouse on a part, the handles are on that part's own box.
        hold(&mut w, false, false, true);
        point_at(&mut w, Vec3::new(-8.0, 14.0, 2.0));
        frame(&mut w);
        assert_eq!(w.resource::<GizmoState>().part, Some(a));
        let part_frame = tools::active_frame(ed(&w), cam(&w), w.resource::<GizmoState>().part).unwrap();
        assert!(part_frame.own_box && (part_frame.bounds.unwrap().half.x - 2.0).abs() < 1e-3);
        // The mouse travels to the part's handle (which stands off its face): the choice stays.
        let handle = part_frame.bounds.unwrap().handle(cam(&w).pos, 0, 1.0);
        point_at(&mut w, handle);
        frame(&mut w);
        assert_eq!(w.resource::<GizmoState>().part, Some(a), "still that part, though the mouse is no longer over it");
        assert_eq!(w.resource::<GizmoState>().hover, Some(crate::tools::Handle::Scale(0, 1.0)));
        mouse(&mut w, false, true, true);
        assert!(w.resource::<GizmoState>().dragging(), "Alt + press on the part's handle takes hold of it");
        assert_eq!(ed(&w).selection, vec![model], "the selection is still the model");
        point_at(&mut w, handle + Vec3::X * 2.0);
        frame(&mut w);
        let dm = ed(&w).dm.clone();
        assert!((dm.borrow().size(a).x - 6.0).abs() < 1e-2, "the part grew: {:?}", dm.borrow().size(a));
        assert_eq!(dm.borrow().size(b), Vec3::splat(4.0), "the other part did not");
        assert_eq!(dm.borrow().parent(a), Some(model), "and it is still inside the model");
        mouse(&mut w, false, true, false);

        // Letting go of Alt puts the handles back on the model, and the part is whole again.
        hold(&mut w, false, false, false);
        frame(&mut w);
        assert_eq!(w.resource::<GizmoState>().part, None);
        assert!(!tools::active_frame(ed(&w), cam(&w), None).unwrap().own_box);
        assert_eq!(ed(&w).selection, vec![model]);
    }
}
