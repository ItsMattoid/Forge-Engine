//! What the debug overlays read: one [`Metrics`] resource, filled in by the collectors in `plugin`
//! (and by the host application, which owns the data model and the script host).
//!
//! Every value is either *measured* (a clock, a counter, the GPU's own timestamps) or *estimated*
//! (worked out from something else); the estimated ones say so in their name or docs and the
//! panels mark them. A number a platform cannot give is `None`, never a made-up zero.

use std::{collections::HashSet, time::Instant};

use bevy::{
    ecs::resource::Resource,
    prelude::{AlphaMode, AssetId, Assets, Mesh},
};

use super::{
    frames::{FrameHistory, GraphPoint, PerfConfig, PerfSnapshot},
    log::Digest,
    overlay::{DebugSettings, Wants},
};
use crate::{
    physics::PhysicsStats,
    render::{AssetStats, PartMaterial},
    scripting::ScriptProfile,
};

/// Names of the rows of [`Timings`] the engine and the editor report.
pub mod timing {
    /// Everything the main thread does in a frame (from the start of the frame to its end).
    pub const MAIN: &str = "Main thread";
    pub const SCRIPTS: &str = "Scripts";
    pub const PHYSICS: &str = "Physics";
    pub const SCENE_SYNC: &str = "Scene sync";
    pub const EDITOR_UI: &str = "Editor UI";
    pub const GIZMOS: &str = "Gizmos";
    pub const INPUT: &str = "Input & camera";
    pub const DEBUG_UI: &str = "Debug overlays";
}

// ---- timings -----------------------------------------------------------------------------------------

/// How fast a row's running average follows the frames (a fraction per frame).
const AVERAGING: f32 = 0.05;

#[derive(Clone, Debug)]
pub struct TimedRow {
    pub name: &'static str,
    /// The last frame, milliseconds.
    pub last_ms: f32,
    /// A running average over roughly the last second.
    pub avg_ms: f32,
    /// The slowest frame between the last two refreshes.
    pub peak_ms: f32,
    accumulating: f32,
    peak_accumulating: f32,
}

/// How long the engine's systems took, by name. Collecting is off unless an overlay needs it; then
/// each system reads the clock twice per frame.
#[derive(Debug, Default)]
pub struct Timings {
    pub enabled: bool,
    rows: Vec<TimedRow>,
}

impl Timings {
    /// Starts a measurement (`None` when timing is off, so the clock is not even read).
    pub fn start(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    /// Ends a measurement started with [`Timings::start`].
    pub fn stop(&mut self, name: &'static str, started: Option<Instant>) {
        if let Some(t) = started {
            self.add_ms(name, t.elapsed().as_secs_f32() * 1000.0);
        }
    }

    /// Adds time to a row for this frame (several additions in a frame sum up).
    pub fn add_ms(&mut self, name: &'static str, ms: f32) {
        if !self.enabled || !ms.is_finite() {
            return;
        }
        match self.rows.iter_mut().find(|r| r.name == name) {
            Some(r) => r.accumulating += ms,
            None => self.rows.push(TimedRow { name, last_ms: 0.0, avg_ms: 0.0, peak_ms: 0.0, accumulating: ms, peak_accumulating: 0.0 }),
        }
    }

    /// Closes the frame: what was added becomes each row's last value.
    pub fn end_frame(&mut self) {
        for r in &mut self.rows {
            r.last_ms = r.accumulating;
            r.accumulating = 0.0;
            r.avg_ms += (r.last_ms - r.avg_ms) * AVERAGING;
            r.peak_accumulating = r.peak_accumulating.max(r.last_ms);
        }
    }

    /// Starts a new peak period (called at each refresh).
    pub fn roll_peaks(&mut self) {
        for r in &mut self.rows {
            r.peak_ms = r.peak_accumulating;
            r.peak_accumulating = 0.0;
        }
    }

    pub fn rows(&self) -> &[TimedRow] {
        &self.rows
    }

    pub fn get(&self, name: &str) -> Option<&TimedRow> {
        self.rows.iter().find(|r| r.name == name)
    }

    pub fn clear(&mut self) {
        self.rows.clear();
    }
}

/// Runs `f` and adds its duration to the row `name` when timing is on (otherwise just runs it, without
/// reading the clock). `metrics` is `None` in an app that has no debug overlays.
pub fn timed<R>(metrics: Option<&mut Metrics>, name: &'static str, f: impl FnOnce() -> R) -> R {
    match metrics {
        Some(m) if m.timings.enabled => {
            let started = Instant::now();
            let result = f();
            m.timings.add_ms(name, started.elapsed().as_secs_f32() * 1000.0);
            result
        }
        _ => f(),
    }
}

// ---- what the renderer draws -------------------------------------------------------------------------

/// Counted from the meshes that are visible this frame (after culling by the camera). Shadow passes
/// and anything the GPU culls on its own are not in these numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneStats {
    /// Mesh entities that exist, and how many of them are visible.
    pub mesh_entities: u32,
    pub visible: u32,
    pub triangles: u64,
    pub vertices: u64,
    /// Estimated draw calls of the main passes: identical (mesh, material) pairs are drawn together
    /// in one instanced call, each see-through object is drawn on its own. Shadows are not included.
    pub draw_calls_est: u32,
    /// Distinct (mesh, material) pairs among the opaque meshes.
    pub batches_est: u32,
    pub transparent: u32,
    pub materials: u32,
    pub meshes: u32,
}

/// Lights that exist (a disabled light has no entity).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LightStats {
    pub point: u32,
    pub spot: u32,
    pub directional: u32,
    /// Of them, the ones that cast shadows.
    pub shadows: u32,
}

impl LightStats {
    pub fn total(&self) -> u32 {
        self.point + self.spot + self.directional
    }
}

/// Reused between counts so nothing is allocated after the first one.
#[derive(Default)]
pub struct TallyScratch {
    pairs: HashSet<(AssetId<Mesh>, Option<AssetId<PartMaterial>>)>,
    materials: HashSet<AssetId<PartMaterial>>,
    meshes: HashSet<AssetId<Mesh>>,
}

/// Counts what the visible meshes add up to. `visible` yields each visible mesh entity's mesh and
/// material.
pub fn tally(
    visible: impl Iterator<Item = (AssetId<Mesh>, Option<AssetId<PartMaterial>>)>,
    meshes: &Assets<Mesh>,
    materials: &Assets<PartMaterial>,
    scratch: &mut TallyScratch,
) -> SceneStats {
    scratch.pairs.clear();
    scratch.materials.clear();
    scratch.meshes.clear();
    let mut stats = SceneStats::default();
    for (mesh, material) in visible {
        stats.visible += 1;
        if let Some(m) = meshes.get(mesh) {
            let vertices = m.count_vertices() as u64;
            stats.vertices += vertices;
            stats.triangles += m.indices().map_or(vertices, |i| i.len() as u64) / 3;
        }
        scratch.meshes.insert(mesh);
        if let Some(material) = material {
            scratch.materials.insert(material);
        }
        let see_through = material.and_then(|id| materials.get(id)).is_some_and(|m| !matches!(m.base.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)));
        if see_through {
            stats.transparent += 1;
        } else {
            scratch.pairs.insert((mesh, material));
        }
    }
    stats.batches_est = scratch.pairs.len() as u32;
    stats.draw_calls_est = stats.batches_est + stats.transparent;
    stats.materials = scratch.materials.len() as u32;
    stats.meshes = scratch.meshes.len() as u32;
    stats
}

// ---- the GPU -------------------------------------------------------------------------------------------

/// One render pass as the GPU's own timing sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PassTime {
    pub name: String,
    pub gpu_ms: Option<f32>,
    pub cpu_ms: Option<f32>,
}

/// What the renderer reports about itself (Bevy's render diagnostics): timestamps and pipeline
/// statistics exist only on Vulkan and DirectX 12, and only when the GPU supports the queries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuStats {
    pub adapter: Option<String>,
    pub backend: Option<String>,
    /// The device was created with timestamp queries.
    pub timestamps: bool,
    /// The device was created with pipeline-statistics queries.
    pub pipeline_statistics: bool,
    /// GPU time of the frame: the sum of the top-level passes (measured).
    pub gpu_ms: Option<f32>,
    /// CPU time spent recording the passes (measured).
    pub render_cpu_ms: Option<f32>,
    /// Passes, slowest first.
    pub passes: Vec<PassTime>,
    pub vertex_invocations: Option<u64>,
    pub primitives: Option<u64>,
    pub fragment_invocations: Option<u64>,
}

/// Folds Bevy's `render/<pass>/<metric>` diagnostics into [`GpuStats`]. `values` yields the full
/// path and the (smoothed) value of every diagnostic that has one.
pub fn fold_gpu(values: impl Iterator<Item = (String, f64)>, into: &mut GpuStats) {
    into.gpu_ms = None;
    into.render_cpu_ms = None;
    into.vertex_invocations = None;
    into.primitives = None;
    into.fragment_invocations = None;
    into.passes.clear();
    let (mut gpu, mut cpu, mut vertices, mut primitives, mut fragments) = (None::<f64>, None::<f64>, None::<f64>, None::<f64>, None::<f64>);
    let add = |acc: &mut Option<f64>, v: f64| *acc = Some(acc.unwrap_or(0.0) + v);
    for (path, value) in values {
        let parts: Vec<&str> = path.split('/').collect();
        // Only top-level passes: `render/<pass>/<metric>` (a nested span is inside one of them).
        if parts.len() != 3 || parts[0] != "render" || !value.is_finite() {
            continue;
        }
        let pass = parts[1];
        let slot = match into.passes.iter().position(|p| p.name == pass) {
            Some(i) => i,
            None => {
                into.passes.push(PassTime { name: pass.to_owned(), ..Default::default() });
                into.passes.len() - 1
            }
        };
        match parts[2] {
            "elapsed_gpu" => {
                add(&mut gpu, value);
                into.passes[slot].gpu_ms = Some(value as f32);
            }
            "elapsed_cpu" => {
                add(&mut cpu, value);
                into.passes[slot].cpu_ms = Some(value as f32);
            }
            "vertex_shader_invocations" => add(&mut vertices, value),
            "clipper_primitives_out" => add(&mut primitives, value),
            "fragment_shader_invocations" => add(&mut fragments, value),
            _ => {}
        }
    }
    into.gpu_ms = gpu.map(|v| v as f32);
    into.render_cpu_ms = cpu.map(|v| v as f32);
    into.vertex_invocations = vertices.map(|v| v as u64);
    into.primitives = primitives.map(|v| v as u64);
    into.fragment_invocations = fragments.map(|v| v as u64);
    into.passes.sort_by(|a, b| b.gpu_ms.unwrap_or(0.0).total_cmp(&a.gpu_ms.unwrap_or(0.0)).then(b.cpu_ms.unwrap_or(0.0).total_cmp(&a.cpu_ms.unwrap_or(0.0))));
}

// ---- memory --------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryStats {
    /// Memory the process holds, bytes (measured by the operating system).
    pub process: Option<u64>,
    pub system_used: Option<u64>,
    pub system_total: Option<u64>,
    /// Vertex and index data of the loaded meshes, and pixel data of the loaded images (counted from
    /// the assets: what the GPU holds for them is about the same, but that is an estimate).
    pub mesh_bytes: u64,
    pub meshes: u32,
    pub image_bytes: u64,
    pub images: u32,
}

// ---- the resource --------------------------------------------------------------------------------------

/// Everything the overlays show.
#[derive(Resource)]
pub struct Metrics {
    /// What is being collected (set from the settings by [`Metrics::apply`]).
    pub wants: Wants,
    pub cfg: PerfConfig,
    pub history_s: f32,
    pub refresh_s: f32,

    pub history: FrameHistory,
    pub perf: PerfSnapshot,
    pub graph: Vec<GraphPoint>,
    pub timings: Timings,

    pub scene: SceneStats,
    pub lights: LightStats,
    pub gpu: GpuStats,
    /// Whether the GPU's own timings are being recorded at all (the plugin was asked to).
    pub gpu_collection: bool,
    pub memory: MemoryStats,
    /// Instances in the data model, and how many are scripts (`None` until the host reports).
    pub instances: Option<(u32, u32)>,
    /// `None`: no script host is running (the editor while not playing).
    pub scripts: Option<ScriptProfile>,
    pub physics: Option<PhysicsStats>,
    pub assets: AssetStats,
    /// Cached files (meshes, materials).
    pub cached: (u32, u32),
    pub log: Digest,

    /// This frame is one the numbers are refreshed in: the host application hands over its (heavier)
    /// data now. Decided at the start of the frame.
    pub sampling: bool,
    /// The user asked to clear the collected data; the host application clears what it owns.
    pub clear_requested: bool,
    pub(super) main_started: Option<Instant>,
    pub(super) since_refresh: f32,
    pub(super) since_graph: f32,
    pub(super) since_memory: f32,
    pub(super) scratch: TallyScratch,
    pub(super) system: Option<Box<sysinfo::System>>,
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics {
            wants: Wants::default(),
            cfg: PerfConfig::default(),
            history_s: 30.0,
            refresh_s: 0.25,
            history: FrameHistory::default(),
            perf: PerfSnapshot::default(),
            graph: Vec::new(),
            timings: Timings::default(),
            scene: SceneStats::default(),
            lights: LightStats::default(),
            gpu: GpuStats::default(),
            gpu_collection: false,
            memory: MemoryStats::default(),
            instances: None,
            scripts: None,
            physics: None,
            assets: AssetStats::default(),
            cached: (0, 0),
            log: Digest::default(),
            sampling: false,
            clear_requested: false,
            main_started: None,
            since_refresh: 0.0,
            since_graph: 0.0,
            since_memory: 0.0,
            scratch: TallyScratch::default(),
            system: None,
        }
    }
}

impl Metrics {
    /// Takes over what the settings ask for. Cheap: call it every frame.
    pub fn apply(&mut self, s: &DebugSettings) {
        self.wants = s.wants();
        self.timings.enabled = self.wants.timings;
        self.cfg = PerfConfig { smoothing_s: s.smoothing_s, average_window_s: s.average_window_s, target_fps: s.target_fps, spike_factor: s.spike_factor };
        self.history_s = s.history_s;
        self.refresh_s = s.refresh_s;
    }

    /// Forgets the history and totals. (The host application clears its own: see `clear_requested`.)
    pub fn clear(&mut self) {
        self.history.clear();
        self.perf = PerfSnapshot::default();
        self.graph.clear();
        self.timings.clear();
        super::log::clear();
        self.log = Digest::default();
        self.clear_requested = true;
    }

    /// Hands over what the script host did in the last frame: cheap, every frame.
    pub fn record_script_frame(&mut self, scripts_ms: f32, physics_ms: f32) {
        self.timings.add_ms(timing::SCRIPTS, scripts_ms);
        self.timings.add_ms(timing::PHYSICS, physics_ms);
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{Cuboid, Sphere};

    use super::*;
    use crate::render::part_material::make_material;

    #[test]
    fn rows_sum_up_within_a_frame_and_average_across_frames() {
        let mut t = Timings { enabled: true, ..Default::default() };
        for _ in 0..200 {
            t.add_ms(timing::SCRIPTS, 1.0);
            t.add_ms(timing::SCRIPTS, 1.0);
            t.end_frame();
        }
        let r = t.get(timing::SCRIPTS).unwrap();
        assert_eq!(r.last_ms, 2.0);
        assert!((r.avg_ms - 2.0).abs() < 0.01, "{}", r.avg_ms);
        // A frame in which the row was not touched: its last value is 0 and the average starts to fall.
        t.end_frame();
        let r = t.get(timing::SCRIPTS).unwrap();
        assert_eq!(r.last_ms, 0.0);
        assert!(r.avg_ms < 2.0 && r.avg_ms > 1.8);
        // The peak is the biggest frame of the period, reported after the period ends.
        assert_eq!(r.peak_ms, 0.0);
        t.roll_peaks();
        assert_eq!(t.get(timing::SCRIPTS).unwrap().peak_ms, 2.0);
    }

    #[test]
    fn timed_blocks_run_either_way_and_only_count_when_timing_is_on() {
        let mut m = Metrics::default();
        assert_eq!(timed(Some(&mut m), timing::SCENE_SYNC, || 7), 7);
        m.timings.end_frame();
        assert!(m.timings.rows().is_empty(), "off: not counted");
        m.timings.enabled = true;
        assert_eq!(timed(Some(&mut m), timing::SCENE_SYNC, || std::thread::sleep(std::time::Duration::from_millis(3))), ());
        m.timings.end_frame();
        assert!(m.timings.get(timing::SCENE_SYNC).unwrap().last_ms >= 2.5, "{:?}", m.timings.rows());
        assert_eq!(timed(None, timing::SCENE_SYNC, || "no overlays in this app"), "no overlays in this app");
    }

    #[test]
    fn timing_does_nothing_while_off() {
        let mut t = Timings::default();
        assert!(t.start().is_none(), "the clock is not even read");
        t.stop(timing::EDITOR_UI, t.start());
        t.add_ms(timing::SCRIPTS, 5.0);
        t.end_frame();
        assert!(t.rows().is_empty());
    }

    fn assets() -> (Assets<Mesh>, Assets<PartMaterial>) {
        (Assets::default(), Assets::default())
    }

    #[test]
    fn the_scene_tally_counts_triangles_vertices_and_batches() {
        let (mut meshes, mut materials) = assets();
        let cube = meshes.add(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)));
        let ball = meshes.add(Mesh::from(Sphere::new(0.5)));
        let (cube_tris, cube_verts) = {
            let m = meshes.get(&cube).unwrap();
            (m.indices().unwrap().len() as u64 / 3, m.count_vertices() as u64)
        };
        let ball_tris = meshes.get(&ball).unwrap().indices().unwrap().len() as u64 / 3;
        let red = materials.add(make_material(&crate::assets::material::MaterialDef::builtin("Plastic"), &Default::default(), false, true));
        let glass = materials.add(make_material(&crate::assets::material::MaterialDef::builtin("Glass"), &Default::default(), true, true));

        // Ten opaque cubes sharing a material (one batch), one ball (a second), two glass cubes (one call each).
        let mut list = Vec::new();
        for _ in 0..10 {
            list.push((cube.id(), Some(red.id())));
        }
        list.push((ball.id(), Some(red.id())));
        list.push((cube.id(), Some(glass.id())));
        list.push((cube.id(), Some(glass.id())));

        let mut scratch = TallyScratch::default();
        let s = tally(list.iter().copied(), &meshes, &materials, &mut scratch);
        assert_eq!(s.visible, 13);
        assert_eq!(s.triangles, cube_tris * 12 + ball_tris);
        assert!(s.vertices >= cube_verts * 12);
        assert_eq!((s.batches_est, s.transparent, s.draw_calls_est), (2, 2, 4));
        assert_eq!((s.materials, s.meshes), (2, 2));
        // Counting again reuses the buffers and gives the same answer.
        assert_eq!(tally(list.iter().copied(), &meshes, &materials, &mut scratch), s);
        assert_eq!(tally(std::iter::empty(), &meshes, &materials, &mut scratch), SceneStats::default());
    }

    #[test]
    fn gpu_numbers_are_the_sums_of_the_top_level_passes_and_absent_when_not_reported() {
        let mut gpu = GpuStats::default();
        fold_gpu(std::iter::empty(), &mut gpu);
        assert_eq!((gpu.gpu_ms, gpu.render_cpu_ms, gpu.vertex_invocations), (None, None, None), "no data is not zero");

        let values = [
            ("render/main_opaque_pass_3d/elapsed_gpu", 2.0),
            ("render/main_opaque_pass_3d/elapsed_cpu", 0.5),
            ("render/main_opaque_pass_3d/vertex_shader_invocations", 1000.0),
            ("render/main_opaque_pass_3d/clipper_primitives_out", 300.0),
            ("render/shadow_pass/elapsed_gpu", 1.0),
            ("render/shadow_pass/elapsed_cpu", 0.25),
            ("render/shadow_pass/inner/elapsed_gpu", 99.0), // nested: already inside shadow_pass
            ("process/mem_usage", 3.0),                      // not a render diagnostic
        ];
        fold_gpu(values.iter().map(|(p, v)| (p.to_string(), *v)), &mut gpu);
        assert_eq!(gpu.gpu_ms, Some(3.0));
        assert_eq!(gpu.render_cpu_ms, Some(0.75));
        assert_eq!((gpu.vertex_invocations, gpu.primitives, gpu.fragment_invocations), (Some(1000), Some(300), None));
        assert_eq!(gpu.passes.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["main_opaque_pass_3d", "shadow_pass"], "slowest first");

        // CPU timings only (a platform without timestamp queries): the GPU time stays absent.
        fold_gpu([("render/main_opaque_pass_3d/elapsed_cpu".to_owned(), 0.5)].into_iter(), &mut gpu);
        assert_eq!((gpu.gpu_ms, gpu.render_cpu_ms), (None, Some(0.5)));
    }

    #[test]
    fn applying_settings_switches_collection_on_and_off() {
        use super::super::overlay::Overlay;
        let mut m = Metrics::default();
        let mut s = DebugSettings::default();
        m.apply(&s);
        assert!(!m.timings.enabled && m.wants == Wants::default());
        s.toggle(Overlay::EngineTimings);
        s.target_fps = 144.0;
        m.apply(&s);
        assert!(m.timings.enabled && m.cfg.target_fps == 144.0);
        s.disable_all();
        m.apply(&s);
        assert!(!m.timings.enabled);
    }
}
