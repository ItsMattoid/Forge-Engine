//! How the editor's window presents frames: V-Sync, a frame-rate limit, anti-aliasing. These are
//! saved with the editor settings (the Rendering tab of the settings window) and applied to the
//! window and the viewport camera every frame they differ.
//!
//! Uncapping the frame rate is two things: V-Sync off (the window no longer waits for the monitor) and no
//! limit (nothing else waits either). The limit is a small frame limiter for people who want a cap
//! below the monitor's rate, or a cap without V-Sync.

use std::time::{Duration, Instant};

use bevy::{
    prelude::*,
    window::{PresentMode, PrimaryWindow},
};
use forge_engine::render::{msaa_from_samples, Culling, WireframeView};
use serde::{Deserialize, Serialize};

use crate::{state::Editor, viewport::EditorCamera};

/// How the window waits for the monitor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum VSync {
    /// Frames are shown in step with the monitor (no tearing; the frame rate is the monitor's).
    #[default]
    On,
    /// Frames are shown as soon as they are ready: the frame rate is not tied to the monitor (tearing is possible).
    Off,
    /// Like V-Sync, but the newest finished frame is shown at each refresh, so the delay is shorter
    /// and the game may render faster than the monitor (where the GPU supports it).
    LowLatency,
}

impl VSync {
    pub const ALL: [VSync; 3] = [VSync::On, VSync::Off, VSync::LowLatency];

    pub fn label(self) -> &'static str {
        match self {
            VSync::On => "On (match the monitor)",
            VSync::Off => "Off (uncapped)",
            VSync::LowLatency => "Low latency (Mailbox)",
        }
    }

    pub fn present_mode(self) -> PresentMode {
        match self {
            VSync::On => PresentMode::AutoVsync,
            VSync::Off => PresentMode::AutoNoVsync,
            VSync::LowLatency => PresentMode::Mailbox,
        }
    }
}

/// Limits of the frame-rate limit, frames per second.
pub const MIN_LIMIT: u32 = 10;
pub const MAX_LIMIT: u32 = 1000;
/// The values anti-aliasing can take (samples per pixel).
pub const MSAA_SAMPLES: [u8; 4] = [1, 2, 4, 8];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderSettings {
    pub vsync: VSync,
    /// Frames per second the editor may not exceed; 0 = no limit.
    pub fps_limit: u32,
    /// Samples per pixel (1 = anti-aliasing off).
    pub msaa: u8,
    /// Skip drawing what is hidden behind other geometry (only on graphics hardware that can do it).
    /// Off by default: see `forge_engine::render::culling` for when it helps.
    pub occlusion_culling: bool,
    /// Draw the edges of every mesh over the scene (where the graphics API can).
    pub wireframe: bool,
    /// With the wireframe on, leave out the shaded parts and draw only the edges.
    pub wireframe_only: bool,
    /// Draw the grid lines on the ground of the editor's viewport. Off by default; only the picture changes
    /// (snapping, the tools and the camera do not depend on it).
    pub show_grid: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        RenderSettings { vsync: VSync::On, fps_limit: 0, msaa: 4, occlusion_culling: false, wireframe: false, wireframe_only: false, show_grid: false }
    }
}

impl RenderSettings {
    /// Puts the numbers in range (a hand-edited file may hold anything).
    pub fn sanitize(&mut self) {
        if self.fps_limit != 0 {
            self.fps_limit = self.fps_limit.clamp(MIN_LIMIT, MAX_LIMIT);
        }
        if !MSAA_SAMPLES.contains(&self.msaa) {
            self.msaa = RenderSettings::default().msaa;
        }
    }

    /// V-Sync off and no limit.
    pub fn uncap(&mut self) {
        self.vsync = VSync::Off;
        self.fps_limit = 0;
    }

    /// Whether nothing holds the frame rate back (neither V-Sync nor a limit).
    pub fn is_uncapped(&self) -> bool {
        self.vsync == VSync::Off && self.fps_limit == 0
    }

    /// The time one frame may take under the limit, if there is one.
    pub fn frame_budget(&self) -> Option<Duration> {
        (self.fps_limit > 0).then(|| Duration::from_secs_f64(1.0 / self.fps_limit as f64))
    }
}

/// Applies the settings to the window and the viewport camera. Cheap: it only writes what differs.
pub fn apply_render_settings(
    ed: NonSend<Editor>,
    mut culling: ResMut<Culling>,
    mut wireframe: ResMut<WireframeView>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Msaa, With<EditorCamera>>,
) {
    let wanted = ed.settings.render;
    if culling.occlusion != wanted.occlusion_culling {
        culling.occlusion = wanted.occlusion_culling;
    }
    if (wireframe.enabled, wireframe.hide_solid) != (wanted.wireframe, wanted.wireframe_only) {
        (wireframe.enabled, wireframe.hide_solid) = (wanted.wireframe, wanted.wireframe_only);
    }
    for mut window in &mut windows {
        if window.present_mode != wanted.vsync.present_mode() {
            window.present_mode = wanted.vsync.present_mode();
        }
    }
    let samples = msaa_from_samples(wanted.msaa);
    for mut msaa in &mut cameras {
        if *msaa != samples {
            *msaa = samples;
        }
    }
}

/// Waits until `target`: sleeps most of the time (a sleep wakes up a little late) and spins the rest.
fn wait_until(target: Instant) {
    const SLACK: Duration = Duration::from_micros(1500);
    let now = Instant::now();
    if let Some(left) = target.checked_duration_since(now) {
        if left > SLACK {
            std::thread::sleep(left - SLACK);
        }
    }
    while Instant::now() < target {
        std::hint::spin_loop();
    }
}

/// When the next frame may start: one frame budget after the last one ended. A frame that ran late
/// is not made up for (the limiter never makes the next frames shorter than the budget).
pub fn next_frame_at(previous_end: Instant, budget: Duration) -> Instant {
    previous_end + budget
}

/// The frame limiter. Runs at the very end of the frame (after the debug collectors have measured
/// it, so waiting shows as "waiting" and not as work).
pub fn limit_frame_rate(ed: NonSend<Editor>, mut previous_end: Local<Option<Instant>>) {
    if let (Some(budget), Some(end)) = (ed.settings.render.frame_budget(), *previous_end) {
        wait_until(next_frame_at(end, budget));
    }
    *previous_end = Some(Instant::now());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_what_the_editor_always_did() {
        let s = RenderSettings::default();
        assert_eq!((s.vsync, s.fps_limit, s.msaa), (VSync::On, 0, 4));
        assert_eq!(s.vsync.present_mode(), PresentMode::AutoVsync);
        assert!(!s.is_uncapped() && s.frame_budget().is_none());
    }

    #[test]
    fn uncapping_turns_vsync_off_and_removes_the_limit() {
        let mut s = RenderSettings { vsync: VSync::LowLatency, fps_limit: 60, msaa: 8, ..Default::default() };
        assert_eq!(s.vsync.present_mode(), PresentMode::Mailbox);
        s.uncap();
        assert!(s.is_uncapped());
        assert_eq!((s.vsync.present_mode(), s.frame_budget(), s.msaa), (PresentMode::AutoNoVsync, None, 8));
    }

    #[test]
    fn numbers_are_kept_in_range() {
        let mut s = RenderSettings { vsync: VSync::On, fps_limit: 3, msaa: 3, ..Default::default() };
        s.sanitize();
        assert_eq!((s.fps_limit, s.msaa), (MIN_LIMIT, 4));
        s.fps_limit = 1_000_000;
        s.sanitize();
        assert_eq!(s.fps_limit, MAX_LIMIT);
        s.fps_limit = 0;
        s.sanitize();
        assert_eq!(s.fps_limit, 0, "0 means no limit and stays that way");
        let parsed: RenderSettings = serde_json::from_str(r#"{ "vsync": "Off", "fps_limit": 144, "from_the_future": 1 }"#).unwrap();
        assert_eq!((parsed.vsync, parsed.fps_limit, parsed.msaa), (VSync::Off, 144, 4), "missing fields take their defaults");
    }

    #[test]
    fn the_budget_is_one_over_the_limit() {
        let s = RenderSettings { fps_limit: 100, ..Default::default() };
        assert_eq!(s.frame_budget(), Some(Duration::from_millis(10)));
    }

    #[test]
    fn the_next_frame_starts_a_budget_after_the_last_one_ended() {
        let end = Instant::now();
        assert_eq!(next_frame_at(end, Duration::from_millis(8)), end + Duration::from_millis(8));
    }

    #[test]
    fn waiting_reaches_the_target_without_overshooting_much() {
        let start = Instant::now();
        let target = start + Duration::from_millis(12);
        wait_until(target);
        let done = Instant::now();
        assert!(done >= target, "never returns early");
        assert!(done - target < Duration::from_millis(4), "overshot by {:?}", done - target);
        // A target in the past returns at once.
        let again = Instant::now();
        wait_until(start);
        assert!(again.elapsed() < Duration::from_millis(2));
    }

    #[test]
    fn the_limiter_holds_the_frame_rate_to_the_limit() {
        // The pacing the limiter system does each frame: wait for one budget after the previous frame ended.
        let frames = 12;
        let budget = Duration::from_millis(10);
        let mut end = Instant::now();
        let t0 = end;
        for _ in 0..frames {
            wait_until(next_frame_at(end, budget));
            end = Instant::now();
        }
        let elapsed = t0.elapsed();
        assert!(elapsed >= budget * frames, "{frames} frames at 100 FPS take at least {:?}, took {elapsed:?}", budget * frames);
        assert!(elapsed < budget * frames + Duration::from_millis(40), "and not much longer: {elapsed:?}");
    }

    #[test]
    fn the_limiter_system_waits_only_when_there_is_a_limit() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_non_send(Editor::new(None));
        // No limit: returns at once, however many times it runs.
        let t0 = Instant::now();
        for _ in 0..20 {
            world.run_system_once(limit_frame_rate).unwrap();
        }
        assert!(t0.elapsed() < Duration::from_millis(50), "{:?}", t0.elapsed());
    }

    #[test]
    fn applying_the_settings_changes_the_window_and_the_camera_and_only_then() {
        use bevy::ecs::system::RunSystemOnce;
        let mut ed = Editor::new(None);
        ed.settings.render = RenderSettings { vsync: VSync::Off, fps_limit: 0, msaa: 2, occlusion_culling: true, wireframe: true, wireframe_only: true, show_grid: false };
        let mut world = World::new();
        world.insert_non_send(ed);
        world.init_resource::<Culling>();
        world.init_resource::<WireframeView>();
        let window = world.spawn((Window::default(), PrimaryWindow)).id();
        let camera = world.spawn((Msaa::Sample4, EditorCamera)).id();
        world.run_system_once(apply_render_settings).unwrap();
        assert_eq!(world.get::<Window>(window).unwrap().present_mode, PresentMode::AutoNoVsync);
        assert_eq!(*world.get::<Msaa>(camera).unwrap(), Msaa::Sample2);
        assert!(world.resource::<Culling>().occlusion, "the culling setting reaches the cameras' culling");
        assert!(world.resource::<WireframeView>().enabled && world.resource::<WireframeView>().hide_solid, "and the wireframe settings reach the renderer");

        world.non_send_mut::<Editor>().settings.render.vsync = VSync::On;
        world.run_system_once(apply_render_settings).unwrap();
        assert_eq!(world.get::<Window>(window).unwrap().present_mode, PresentMode::AutoVsync);
    }
}
