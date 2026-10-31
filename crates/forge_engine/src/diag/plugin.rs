//! The collectors: Bevy systems that record frame times every frame and, at the refresh rate of the
//! overlay, count what the renderer draws, read the GPU's measurements and ask the operating system for
//! memory. Everything beyond the frame times is only done while an overlay needs it (see `Wants`).
//!
//! The host application (the editor, the game runtime) adds what only it can see: the data model's
//! instance count, the script host's timings, the physics statistics. It also calls
//! [`Metrics::apply`] with its settings every frame.

use bevy::{
    diagnostic::DiagnosticsStore,
    ecs::system::SystemParam,
    camera::visibility::ViewVisibility,
    light::{DirectionalLight, PointLight, SpotLight},
    pbr::MeshMaterial3d,
    prelude::*,
    render::{
        diagnostic::RenderDiagnosticsPlugin,
        renderer::{RenderAdapterInfo, RenderDevice},
        settings::WgpuFeatures,
    },
    time::{Real, TimeSystems},
};

use super::{
    log,
    metrics::{fold_gpu, tally, timing, LightStats, Metrics},
};
use crate::render::{AssetCache, PartMaterial};

/// Columns of the graphs (a graph is rarely wider than this many pixels).
const GRAPH_COLUMNS: usize = 240;
/// Memory is asked of the operating system this often (seconds): it is the slowest thing collected.
const MEMORY_EVERY: f32 = 1.0;

/// The system that closes a frame's measurements; anything that should not count as work (a frame
/// limiter that sleeps) goes `.after(DiagEnd)`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiagEnd;

pub struct DiagPlugin {
    /// Also record the GPU's own timings (Bevy's render diagnostics). On Vulkan and DirectX 12 this
    /// costs timestamp writes around every pass and a buffer read-back per frame - in Bevy 0.19 about half a
    /// millisecond of an editor frame - so a host should only ask for it when an overlay shows the numbers
    /// (DebugSettings::wants().gpu). It cannot be switched on later: the recorder is set up with the renderer.
    pub gpu_timing: bool,
}

impl Plugin for DiagPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Metrics { gpu_collection: self.gpu_timing, ..Default::default() })
            .add_systems(First, begin_frame.after(TimeSystems))
            .add_systems(Last, end_frame.in_set(DiagEnd));
        if self.gpu_timing {
            app.add_plugins(RenderDiagnosticsPlugin);
        }
    }
}

/// Records the length of the previous frame and starts the clock for this one.
fn begin_frame(time: Res<Time<Real>>, mut m: ResMut<Metrics>) {
    let dt = time.delta_secs();
    m.history.push(dt);
    m.main_started = m.timings.start();
    m.since_refresh += dt;
    m.since_graph += dt;
    m.since_memory += dt;
    m.sampling = m.since_refresh >= m.refresh_s;
}

/// What the end-of-frame collector reads. (Optional where a minimal app may not have it.)
#[derive(SystemParam)]
struct Sources<'w, 's> {
    visible: Query<'w, 's, (&'static ViewVisibility, &'static Mesh3d, Option<&'static MeshMaterial3d<PartMaterial>>)>,
    point: Query<'w, 's, &'static PointLight>,
    spot: Query<'w, 's, &'static SpotLight>,
    directional: Query<'w, 's, &'static DirectionalLight>,
    meshes: Option<Res<'w, Assets<Mesh>>>,
    materials: Option<Res<'w, Assets<PartMaterial>>>,
    images: Option<Res<'w, Assets<Image>>>,
    diagnostics: Option<Res<'w, DiagnosticsStore>>,
    device: Option<Res<'w, RenderDevice>>,
    adapter: Option<Res<'w, RenderAdapterInfo>>,
    cache: Option<Res<'w, AssetCache>>,
}

fn end_frame(mut metrics: ResMut<Metrics>, src: Sources) {
    let m = &mut *metrics;
    if let Some(t) = m.main_started.take() {
        m.timings.add_ms(timing::MAIN, t.elapsed().as_secs_f32() * 1000.0);
    }
    m.timings.end_frame();

    if m.wants.graph && m.since_graph >= m.refresh_s.min(0.1) {
        m.since_graph = 0.0;
        m.history.graph(m.history_s, GRAPH_COLUMNS, &m.perf, &mut m.graph);
    }
    if !m.sampling {
        return;
    }
    m.since_refresh = 0.0;
    m.timings.roll_peaks();

    if m.wants.frames {
        m.perf = m.history.snapshot(&m.cfg);
    }
    if m.wants.scene {
        if let (Some(meshes), Some(materials)) = (&src.meshes, &src.materials) {
            let visible = src.visible.iter().filter(|(v, ..)| v.get()).map(|(_, mesh, material)| (mesh.0.id(), material.map(|m| m.0.id())));
            let mut stats = tally(visible, meshes, materials, &mut m.scratch);
            stats.mesh_entities = src.visible.iter().count() as u32;
            m.scene = stats;
        }
    }
    if m.wants.lights {
        let shadows = src.point.iter().filter(|l| l.shadow_maps_enabled).count() + src.spot.iter().filter(|l| l.shadow_maps_enabled).count() + src.directional.iter().filter(|l| l.shadow_maps_enabled).count();
        m.lights = LightStats { point: src.point.iter().count() as u32, spot: src.spot.iter().count() as u32, directional: src.directional.iter().count() as u32, shadows: shadows as u32 };
    }
    if m.wants.gpu {
        collect_gpu(m, &src);
    }
    if m.wants.memory && m.since_memory >= MEMORY_EVERY {
        m.since_memory = 0.0;
        collect_memory(m, &src);
    }
    if m.wants.assets {
        if let Some(cache) = &src.cache {
            m.assets = cache.stats.clone();
            m.cached = (cache.counts().0 as u32, cache.counts().1 as u32);
        }
    }
    if m.wants.log {
        m.log = log::digest();
    }
}

fn collect_gpu(m: &mut Metrics, src: &Sources) {
    if let Some(adapter) = &src.adapter {
        m.gpu.adapter = Some(adapter.name.clone());
        m.gpu.backend = Some(format!("{:?}", adapter.backend));
    }
    if let Some(device) = &src.device {
        let features = device.features();
        m.gpu.timestamps = features.contains(WgpuFeatures::TIMESTAMP_QUERY);
        m.gpu.pipeline_statistics = features.contains(WgpuFeatures::PIPELINE_STATISTICS_QUERY);
    }
    if !m.gpu_collection {
        return;
    }
    if let Some(store) = &src.diagnostics {
        let values = store.iter().filter(|d| d.path().as_str().starts_with("render/")).filter_map(|d| d.smoothed().map(|v| (d.path().as_str().to_owned(), v)));
        fold_gpu(values, &mut m.gpu);
    }
}

fn collect_memory(m: &mut Metrics, src: &Sources) {
    let system = m.system.get_or_insert_with(|| Box::new(sysinfo::System::new()));
    system.refresh_memory();
    m.memory.system_total = Some(system.total_memory()).filter(|v| *v > 0);
    m.memory.system_used = Some(system.used_memory()).filter(|_| m.memory.system_total.is_some());
    m.memory.process = sysinfo::get_current_pid().ok().and_then(|pid| {
        system.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[pid]), true, sysinfo::ProcessRefreshKind::nothing().with_memory());
        system.process(pid).map(|p| p.memory())
    });

    if let Some(meshes) = &src.meshes {
        let (mut bytes, mut count) = (0u64, 0u32);
        for (_, mesh) in meshes.iter() {
            bytes += (mesh.get_vertex_buffer_size() + mesh.get_index_buffer_bytes().map_or(0, |b| b.len())) as u64;
            count += 1;
        }
        m.memory.mesh_bytes = bytes;
        m.memory.meshes = count;
    }
    if let Some(images) = &src.images {
        let (mut bytes, mut count) = (0u64, 0u32);
        for (_, image) in images.iter() {
            bytes += image.data.as_ref().map_or(0, |d| d.len()) as u64;
            count += 1;
        }
        m.memory.image_bytes = bytes;
        m.memory.images = count;
    }
}
