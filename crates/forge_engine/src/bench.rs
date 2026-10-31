//! `forge_player --bench <frames> [--screenshot <png>]`: plays a game for a fixed number of frames, reports
//! how long they took, and optionally saves what the window showed.
//!
//! This is the whole-engine counterpart of `tests/perf.rs`: scripts, physics, the render mirror, Bevy's
//! extraction and the GPU all take part, so it measures what the benchmarks without a window cannot (draw
//! calls, culling, shading). V-Sync is switched off for the run, or every frame would take as long as the
//! monitor's refresh, and the window is redrawn continuously whether or not it has the focus. The first
//! [`WARMUP`] frames are not counted: the scene is loading, scripts are starting and the GPU is still
//! compiling pipelines.
//!
//! The report is one line on stdout, e.g. `bench: 600 frames | avg 4.12 ms (242.7 fps) | p50 ... | p99 ...`.

use std::path::PathBuf;

use bevy::{
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
    window::{PresentMode, PrimaryWindow},
    winit::{UpdateMode, WinitSettings},
};

use crate::{
    compat::{self, ExitWriter},
    render::Culling,
};

/// Frames that are played but not measured.
pub const WARMUP: u32 = 180;
/// Frames to wait for the screenshot file before giving up.
const SCREENSHOT_PATIENCE: u32 = 300;

/// What a benchmark run does.
#[derive(Resource, Clone, Debug)]
pub struct BenchPlan {
    /// Frames to measure after the warm-up (0: measure nothing).
    pub frames: u32,
    /// A PNG to save after the last measured frame.
    pub screenshot: Option<PathBuf>,
}

#[derive(Resource, Default)]
struct BenchRun {
    /// Frames played so far, warm-up included.
    played: u32,
    /// Milliseconds each measured frame took.
    ms: Vec<f32>,
    /// The frame the screenshot was asked for.
    shot_at: Option<u32>,
}

/// Makes the app run `plan` and exit. Call after [`crate::runtime::ForgeRuntimePlugin`] and the default plugins.
pub fn add_benchmark(app: &mut App, plan: BenchPlan) {
    // A window that is not in front is by default only redrawn 60 times a second, which would cap
    // (and so change) the numbers whenever the window happens not to have the focus.
    app.insert_resource(WinitSettings { focused_mode: UpdateMode::Continuous, unfocused_mode: UpdateMode::Continuous })
        .insert_resource(plan)
        .init_resource::<BenchRun>()
        .add_systems(Update, run_benchmark);
}

fn run_benchmark(
    plan: Res<BenchPlan>,
    mut run: ResMut<BenchRun>,
    time: Res<Time<Real>>,
    culling: Res<Culling>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut commands: Commands,
    mut exit: ExitWriter,
) {
    run.played += 1;
    if run.played == 1 {
        for mut window in &mut windows {
            window.present_mode = PresentMode::AutoNoVsync;
        }
    }
    if run.played > WARMUP && (run.ms.len() as u32) < plan.frames {
        run.ms.push(time.delta_secs() * 1000.0);
        if run.ms.len() as u32 == plan.frames {
            println!("{}", summary(&run.ms, &culling));
        }
    }
    if run.played <= WARMUP || (run.ms.len() as u32) < plan.frames {
        return;
    }
    let Some(path) = &plan.screenshot else { return compat::request_exit(&mut exit) };
    match run.shot_at {
        None => {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
            run.shot_at = Some(run.played);
        }
        // The file is written a few frames after it was asked for.
        Some(at) if path.is_file() || run.played - at > SCREENSHOT_PATIENCE => {
            println!("screenshot: {}", if path.is_file() { path.display().to_string() } else { "not written".into() });
            compat::request_exit(&mut exit);
        }
        Some(_) => {}
    }
}

/// One line: how the measured frames went, and whether occlusion culling was in use.
fn summary(ms: &[f32], culling: &Culling) -> String {
    let mut sorted = ms.to_vec();
    sorted.sort_by(f32::total_cmp);
    let at = |q: f32| sorted[((sorted.len() - 1) as f32 * q).round() as usize];
    let avg = ms.iter().sum::<f32>() / ms.len() as f32;
    let occlusion = match (culling.occlusion, culling.supported) {
        (false, _) => "off (setting)",
        (true, false) => "unavailable on this graphics API",
        (true, true) => "on",
    };
    format!(
        "bench: {} frames | avg {avg:.2} ms ({:.1} fps) | p50 {:.2} | p99 {:.2} | max {:.2} | occlusion culling: {occlusion}",
        ms.len(),
        1000.0 / avg,
        at(0.5),
        at(0.99),
        at(1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_reports_the_average_the_percentiles_and_the_culling_state() {
        let ms: Vec<f32> = (1..=100).map(|i| i as f32).collect();
        let line = summary(&ms, &Culling { occlusion: true, supported: true });
        assert!(line.starts_with("bench: 100 frames | avg 50.50 ms (19.8 fps) | p50 51.00 | p99 99.00 | max 100.00"), "{line}");
        assert!(line.ends_with("occlusion culling: on"));
        assert!(summary(&ms, &Culling { occlusion: true, supported: false }).contains("unavailable"));
        assert!(summary(&ms, &Culling { occlusion: false, supported: true }).contains("off"));
    }
}
