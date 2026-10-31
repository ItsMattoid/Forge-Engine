//! Forge editor entry point: `forge_editor [project-folder]`.

mod dock;
mod gizmo;
mod keymap;
mod play;
mod render_settings;
mod settings;
mod state;
mod theme;
mod tools;
mod ui;
mod viewport;

use std::path::PathBuf;

use bevy::{log::LogPlugin, prelude::*};
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};
use forge_engine::{diag::DiagPlugin, render::AssetCache};

fn main() {
    let initial = std::env::args().nth(1).map(PathBuf::from);
    // (Made before the app, because its saved settings decide what the diagnostics record.)
    let editor = state::Editor::new(initial);

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window { title: "Forge Editor".into(), resolution: (1600, 900).into(), ..default() }),
                    ..default()
                })
                // The Warnings & Errors overlay lists what the engine, the renderer and scripts report.
                .set(LogPlugin { custom_layer: forge_engine::diag::log::layer, ..default() })
                // (The engine mixes its own sounds, see orge_engine::audio; Bevy's audio plugin would open a second output.)
                .disable::<bevy::audio::AudioPlugin>(),
        )
        // bevy_egui 0.42. Its context belongs to a camera of its own that covers the whole window (the 3D viewport's
        // camera only covers the viewport panel), made in `viewport::spawn_editor_camera`; the UI is drawn by
        // systems in `EguiPrimaryContextPass`, which runs after `Update` (see `ui::ui_system`).
        .insert_resource(EguiGlobalSettings { auto_create_primary_context: false, ..default() })
        .add_plugins(EguiPlugin::default())
        // Frame times are always recorded; everything else only while a Debug overlay needs it. The GPU's own
        // timings (Vulkan / DirectX 12) cost a few timestamp writes per pass.
        .add_plugins(DiagPlugin { gpu_timing: editor.settings.debug.wants().gpu })
        // Parts are drawn with one shared material per surface kind and their colour per instance, so any number
        // of parts of a shape cost one draw call (see `forge_engine::render::part_material`).
        .add_plugins(forge_engine::render::PartMaterialPlugin)
        // The physics world and its fixed-rate step. While a game plays, the two systems keep the scene and the world in
        // step (see play); otherwise they do nothing and the world stays empty.
        .add_plugins(forge_engine::physics::ForgePhysicsPlugin)
        .add_systems(FixedUpdate, (play::physics_pre_step.in_set(forge_engine::physics::Box3dSet::Sync), play::physics_post_step.after(forge_engine::physics::Box3dSet::Step)))
        .init_resource::<AssetCache>()
        .init_resource::<viewport::ViewportRect>()
        .init_resource::<viewport::EditorCam>()
        .init_resource::<tools::GizmoState>()
        .init_gizmo_group::<tools::ToolGizmos>()
        .insert_non_send(editor)
        .add_systems(Startup, (viewport::spawn_editor_camera, tools::configure_gizmos))
        .add_systems(
            Update,
            (
                ui::save_on_close,
                play::play_frame,
                ui::debug_feed,
                viewport::camera_controls,
                viewport::viewport_cursor,
                play::apply_play_camera,
                play::apply_play_window,
                viewport::pick_and_move,
                viewport::sync_scene,
                viewport::apply_viewport,
                viewport::draw_gizmos,
                tools::draw_tool_lines,
                viewport::drop_assets,
                ui::os_file_drops,
                ui::commit_edits,
                render_settings::apply_render_settings,
            )
                .chain(),
        )
        // The editor's UI: it sets what the systems above read next frame (where the viewport is, what the mouse is over).
        .add_systems(EguiPrimaryContextPass, (ui::ui_system, tools::draw_overlay).chain())
        // The frame limiter goes last, after the debug collectors have measured the frame.
        .add_systems(Last, render_settings::limit_frame_rate.after(forge_engine::diag::DiagEnd))
        .add_systems(Update, bench_frames)
        .insert_resource(bevy::winit::WinitSettings { focused_mode: bevy::winit::UpdateMode::Continuous, unfocused_mode: bevy::winit::UpdateMode::Continuous })
        .run();
}

/// `FORGE_EDITOR_BENCH=<frames>`: measures the editor's frame time (V-Sync and the frame limit off) over that many frames
/// after a warm-up, prints one line and exits. The editor's counterpart of `forge_player --bench`.
fn bench_frames(mut exit: MessageWriter<AppExit>, mut ed: NonSendMut<state::Editor>, mut state: Local<(u32, Option<std::time::Instant>, Vec<f32>)>) {
    const WARMUP: u32 = 300;
    let Some(frames) = std::env::var("FORGE_EDITOR_BENCH").ok().and_then(|s| s.parse::<u32>().ok()) else { return };
    ed.settings.render.uncap();
    let (n, last, ms) = &mut *state;
    *n += 1;
    let now = std::time::Instant::now();
    if *n > WARMUP {
        if let Some(prev) = *last {
            ms.push((now - prev).as_secs_f32() * 1000.0);
        }
    }
    *last = Some(now);
    if ms.len() as u32 >= frames {
        let mut sorted = ms.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let avg = ms.iter().sum::<f32>() / ms.len() as f32;
        println!("editor bench: {} frames | avg {:.3} ms ({:.1} fps) | p50 {:.3} | p99 {:.3}", ms.len(), avg, 1000.0 / avg, sorted[sorted.len() / 2], sorted[sorted.len() * 99 / 100]);
        exit.write(AppExit::Success);
    }
}