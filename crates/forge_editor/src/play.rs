//! The systems that run a game inside the viewport (see `state::play` for starting and stopping).
//!
//! Each frame: `play_frame` hands the keyboard and mouse to the game and steps it, `apply_play_camera`
//! points the viewport camera through the game's camera, and `apply_play_window` captures the
//! mouse when the game asks for it.

use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use forge_engine::{
    physics::{Box3dStats, Box3dWorld},
    scripting::FrameInput,
};

use crate::{
    state::Editor,
    viewport::{EditorCam, EditorCamera, ViewportRect},
};

/// Steps the running game (scripts, physics, events). Does nothing when no game is playing.
pub fn play_frame(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    vp: Res<ViewportRect>,
    physics: NonSend<Box3dWorld>,
    mut ed: NonSendMut<Editor>,
) {
    let ed = &mut *ed;
    // The game only sees the keyboard and mouse while the viewport has the focus; otherwise
    // typing in a text field or flying the editor camera would also move the character.
    let forward = ed.game_has_input() && !vp.typing;
    let (no_keys, no_mouse) = (ButtonInput::<KeyCode>::default(), ButtonInput::<MouseButton>::default());
    let wheel = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    let input = if forward {
        FrameInput {
            dt: time.delta_secs(),
            keys: &keys,
            mouse: &mouse,
            mouse_delta: motion.delta,
            cursor: Editor::viewport_cursor(vp.pointer, vp.min_pt, vp.max_pt),
            wheel,
        }
    } else {
        FrameInput { dt: time.delta_secs(), keys: &no_keys, mouse: &no_mouse, mouse_delta: Vec2::ZERO, cursor: None, wheel: 0.0 }
    };
    ed.tick_play_with(&input, Some(physics.world()));

    // Escape gives the keyboard back to the editor (the game still saw the key press this frame).
    if forward && keys.just_pressed(KeyCode::Escape) {
        if let Some(play) = ed.play.as_mut() {
            play.focused = false;
        }
    }
}

/// Before each physics step (in `FixedUpdate`): what the running game's scripts and the editor changed since the last
/// one is applied to the world. Does nothing when no game is playing.
pub fn physics_pre_step(ed: NonSend<Editor>, physics: NonSend<Box3dWorld>, time: Res<Time<Fixed>>) {
    if let Some(play) = ed.play.as_ref() {
        play.host.physics_pre_step(physics.world(), time.delta_secs());
    }
}

/// After each physics step: where things moved to goes back into the scene, touches become events.
pub fn physics_post_step(ed: NonSend<Editor>, physics: NonSend<Box3dWorld>, stats: Res<Box3dStats>) {
    if let Some(play) = ed.play.as_ref() {
        play.host.physics_post_step(physics.world(), stats.step_ms as f32);
    }
}

/// While looking through the game's camera, the viewport camera follows `Workspace.CurrentCamera`.
/// Otherwise it keeps the editor camera's field of view.
pub fn apply_play_camera(ed: NonSend<Editor>, cam: Res<EditorCam>, mut cameras: Query<(&mut Transform, &mut Projection), With<EditorCamera>>) {
    let through_game = ed.play.as_ref().is_some_and(|p| p.game_camera);
    let game_view = through_game
        .then(|| {
            let dm = ed.dm.borrow();
            let camera = dm.reference(dm.workspace, "CurrentCamera")?;
            let (pos, rot) = dm.world_cframe(camera)?;
            Some((pos, rot, dm.num(camera, "FieldOfView", 70.0).clamp(1.0, 120.0).to_radians()))
        })
        .flatten();

    for (mut transform, mut projection) in &mut cameras {
        let fov = match game_view {
            Some((pos, rot, fov)) => {
                transform.translation = pos;
                transform.rotation = rot;
                fov
            }
            None => cam.fov,
        };
        if let Projection::Perspective(p) = &mut *projection {
            if p.fov != fov {
                p.fov = fov;
            }
        }
    }
}

/// Captures the mouse (hidden, kept in the viewport) while the game asks for `LockCenter`.
pub fn apply_play_window(ed: NonSend<Editor>, vp: Res<ViewportRect>, mut windows: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>, mut captured: Local<bool>) {
    let Ok((mut window, mut cursor)) = windows.single_mut() else { return };
    let wanted = ed.play.as_ref().is_some_and(|p| p.takes_input() && matches!(p.host.mouse_behavior().as_str(), "LockCenter" | "LockCurrentPosition")) && window.focused;

    if wanted != *captured {
        cursor.grab_mode = if wanted { CursorGrabMode::Locked } else { CursorGrabMode::None };
        cursor.visible = !wanted;
        *captured = wanted;
    }
    if wanted {
        // Where the pointer sits is irrelevant to the game while captured, but it must stay
        // inside the viewport so a click does not land on a panel and take the focus away.
        window.set_cursor_position(Some((vp.min_pt + vp.max_pt) * 0.5));
    }
}
