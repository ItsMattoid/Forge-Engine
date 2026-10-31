//! What the debug menu can switch on, and the settings that go with it.
//!
//! Every overlay is one row of [`OVERLAYS`]: a stable id (used in settings files and for the shortcut
//! in the editor), a name, the group it is listed under and what it shows. The Debug menu, the
//! Shortcuts tab and the panels are all generated from that table, so a new overlay is one enum
//! variant, one row here and a drawing function in `ui`.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Something that can be shown over the viewport.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Overlay {
    // Performance
    Fps,
    FpsGraph,
    FrameTime,
    FrameTimeGraph,
    FrameBudget,
    OnePercentLow,
    FrameBreakdown,
    // Rendering and scene
    InstanceCount,
    DrawCalls,
    Triangles,
    Vertices,
    Lights,
    RenderStats,
    // Scripting and engine
    ScriptPerformance,
    EngineTimings,
    // Physics
    Physics,
    // Diagnostics
    Memory,
    Network,
    AssetLoading,
    Warnings,
}

/// The groups of the Debug menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Performance,
    Rendering,
    Scripting,
    Physics,
    Diagnostics,
}

impl Group {
    pub const ALL: [Group; 5] = [Group::Performance, Group::Rendering, Group::Scripting, Group::Physics, Group::Diagnostics];

    pub fn label(self) -> &'static str {
        match self {
            Group::Performance => "Performance",
            Group::Rendering => "Rendering",
            Group::Scripting => "Scripting",
            Group::Physics => "Physics",
            Group::Diagnostics => "Diagnostics",
        }
    }
}

/// One row of the table of overlays.
pub struct OverlayDef {
    pub overlay: Overlay,
    /// Stable name used in saved settings.
    pub id: &'static str,
    /// The id of the overlay's shortcut in the editor (`debug.` + id).
    pub key: &'static str,
    pub name: &'static str,
    pub group: Group,
    pub description: &'static str,
    /// The starting shortcut (see the editor's keymap); empty = unbound.
    pub shortcut: &'static [&'static str],
}

macro_rules! row {
    ($overlay:expr, $id:literal, $name:literal, $group:expr, $description:literal) => {
        OverlayDef { overlay: $overlay, id: $id, key: concat!("debug.", $id), name: $name, group: $group, description: $description, shortcut: &[] }
    };
}

/// Every overlay, in the order the menu lists them. The position of a row is its variant's number.
pub static OVERLAYS: &[OverlayDef] = &[
    OverlayDef { shortcut: &["Ctrl+Shift+F5"], ..row!(Overlay::Fps, "fps", "Show FPS", Group::Performance, "The frame rate, smoothed so it can be read (the raw value of the last frame is shown next to it).") },
    row!(Overlay::FpsGraph, "fps_graph", "FPS Graph", Group::Performance, "The frame rate over time: stutters and sustained drops stand out, with 30 / 60 / 120 / 144 FPS reference lines."),
    row!(Overlay::FrameTime, "frame_time", "Frame Time", Group::Performance, "How long the last frames took, in milliseconds (current, average and worst)."),
    row!(Overlay::FrameTimeGraph, "frame_time_graph", "Frame Time Graph", Group::Performance, "Frame times over time. Uneven frame pacing and lag spikes show as tall bars."),
    row!(Overlay::FrameBudget, "frame_budget", "Frame Budget", Group::Performance, "The time one frame may take to reach the target frame rate, how much of it is used, and how many frames went over."),
    row!(Overlay::OnePercentLow, "one_percent_low", "1% Low FPS", Group::Performance, "The average frame rate of the slowest 1% of frames: a measure of consistency."),
    row!(Overlay::FrameBreakdown, "frame_breakdown", "Frame Time Breakdown", Group::Performance, "Where a frame's time goes: scripts, physics, scene updates, the editor, rendering on the CPU and on the GPU (where measurable), and waiting."),
    row!(Overlay::InstanceCount, "instances", "Show Instance Count", Group::Rendering, "How many instances the data model holds, and how many of them are drawn."),
    row!(Overlay::DrawCalls, "draw_calls", "Show Draw Calls", Group::Rendering, "Draw calls per frame (an estimate from the visible meshes and materials)."),
    row!(Overlay::Triangles, "triangles", "Show Triangle Count", Group::Rendering, "Triangles of the meshes that are drawn."),
    row!(Overlay::Vertices, "vertices", "Show Vertex Count", Group::Rendering, "Vertices of the meshes that are drawn."),
    row!(Overlay::Lights, "lights", "Show Active Lights", Group::Rendering, "Lights that exist in the scene by kind, and how many of them cast shadows."),
    row!(Overlay::RenderStats, "render_stats", "Show Render Statistics", Group::Rendering, "Batches, materials, meshes, and (where the GPU can tell) per-pass GPU time and shader activity."),
    row!(Overlay::ScriptPerformance, "scripts", "Show Script Performance", Group::Scripting, "How long scripts take, per phase of the frame and per script, most expensive first."),
    row!(Overlay::EngineTimings, "engine_timings", "Show Engine Timings", Group::Scripting, "Time spent by the major engine systems each frame."),
    row!(Overlay::Physics, "physics", "Show Physics Statistics", Group::Physics, "Physics step time, simulated and awake bodies, static colliders and touches."),
    row!(Overlay::Memory, "memory", "Show Memory Usage", Group::Diagnostics, "Memory the program uses, and what its meshes and textures weigh."),
    row!(Overlay::Network, "network", "Show Network Statistics", Group::Diagnostics, "Ping, traffic and packet loss. Forge has no networking yet, so this says so."),
    row!(Overlay::AssetLoading, "assets", "Show Asset Loading", Group::Diagnostics, "Files loaded, how long they took, and what failed."),
    row!(Overlay::Warnings, "warnings", "Show Warnings & Errors", Group::Diagnostics, "Warnings and errors reported while running (the engine's, the renderer's and scripts')."),
];

impl Overlay {
    pub fn def(self) -> &'static OverlayDef {
        &OVERLAYS[self as usize]
    }

    pub fn from_id(id: &str) -> Option<Overlay> {
        OVERLAYS.iter().find(|d| d.id == id).map(|d| d.overlay)
    }

    /// What has to be collected for this overlay to have something to show.
    fn wants(self) -> Wants {
        let mut w = Wants::default();
        match self {
            Overlay::Fps | Overlay::FrameTime | Overlay::FrameBudget | Overlay::OnePercentLow => w.frames = true,
            Overlay::FpsGraph | Overlay::FrameTimeGraph => {
                w.frames = true;
                w.graph = true;
            }
            Overlay::FrameBreakdown => {
                w.frames = true;
                w.timings = true;
                w.gpu = true;
                w.scripts = true;
                w.physics = true;
            }
            Overlay::InstanceCount => {
                w.instances = true;
                w.scene = true;
            }
            Overlay::DrawCalls | Overlay::Triangles | Overlay::Vertices => w.scene = true,
            Overlay::Lights => w.lights = true,
            Overlay::RenderStats => {
                w.scene = true;
                w.gpu = true;
            }
            Overlay::ScriptPerformance => {
                w.scripts = true;
                w.script_costs = true;
            }
            Overlay::EngineTimings => {
                w.timings = true;
                w.scripts = true;
                w.physics = true;
            }
            Overlay::Physics => w.physics = true,
            Overlay::Memory => w.memory = true,
            Overlay::AssetLoading => w.assets = true,
            Overlay::Warnings => w.log = true,
            Overlay::Network => {}
        }
        w
    }
}

/// What the collectors have to do right now. Everything is off unless an overlay needs it, so a
/// closed Debug menu costs next to nothing (the frame times are the only thing always recorded).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Wants {
    /// Work out the frame-rate statistics (any overlay of the performance panel).
    pub frames: bool,
    /// Build the points of the graphs.
    pub graph: bool,
    /// Time the engine's own systems (the "timings" rows) and the main thread.
    pub timings: bool,
    /// Count what the renderer draws (visible meshes, triangles, batches).
    pub scene: bool,
    /// Count the data model's instances.
    pub instances: bool,
    /// Count lights.
    pub lights: bool,
    /// Keep the recorded warnings and errors up to date.
    pub log: bool,
    /// Read the GPU's own measurements.
    pub gpu: bool,
    /// Ask the operating system for the memory in use.
    pub memory: bool,
    /// Copy the script host's phase times.
    pub scripts: bool,
    /// Measure every script's cost (adds clock reads to each handler).
    pub script_costs: bool,
    pub physics: bool,
    pub assets: bool,
}

impl Wants {
    fn union(self, o: Wants) -> Wants {
        Wants {
            frames: self.frames || o.frames,
            graph: self.graph || o.graph,
            timings: self.timings || o.timings,
            scene: self.scene || o.scene,
            instances: self.instances || o.instances,
            lights: self.lights || o.lights,
            log: self.log || o.log,
            gpu: self.gpu || o.gpu,
            memory: self.memory || o.memory,
            scripts: self.scripts || o.scripts,
            script_costs: self.script_costs || o.script_costs,
            physics: self.physics || o.physics,
            assets: self.assets || o.assets,
        }
    }
}

// ---- the set of enabled overlays -------------------------------------------------------------------

/// Which overlays are on. Several may be on at once.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct OverlaySet(u32);

impl OverlaySet {
    pub fn contains(self, o: Overlay) -> bool {
        self.0 & (1 << o as u32) != 0
    }

    pub fn set(&mut self, o: Overlay, on: bool) {
        if on {
            self.0 |= 1 << o as u32;
        } else {
            self.0 &= !(1 << o as u32);
        }
    }

    /// Flips an overlay; returns whether it is on now.
    pub fn toggle(&mut self, o: Overlay) -> bool {
        let on = !self.contains(o);
        self.set(o, on);
        on
    }

    pub fn clear(&mut self) {
        self.0 = 0;
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn iter(self) -> impl Iterator<Item = Overlay> {
        OVERLAYS.iter().map(|d| d.overlay).filter(move |o| self.contains(*o))
    }
}

/// Saved as a list of ids; ids this version does not know are skipped.
impl Serialize for OverlaySet {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.iter().map(|o| o.def().id))
    }
}

impl<'de> Deserialize<'de> for OverlaySet {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let ids = Vec::<String>::deserialize(d)?;
        let mut set = OverlaySet::default();
        for id in ids {
            if let Some(o) = Overlay::from_id(&id) {
                set.set(o, true);
            }
        }
        Ok(set)
    }
}

// ---- placement ---------------------------------------------------------------------------------------

/// Where a panel sits in the viewport.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Dock {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// Wherever it was dragged to (`Placement::offset` from the viewport's top-left corner).
    Free,
}

impl Dock {
    pub const ALL: [Dock; 5] = [Dock::TopLeft, Dock::TopRight, Dock::BottomLeft, Dock::BottomRight, Dock::Free];

    pub fn label(self) -> &'static str {
        match self {
            Dock::TopLeft => "Top left",
            Dock::TopRight => "Top right",
            Dock::BottomLeft => "Bottom left",
            Dock::BottomRight => "Bottom right",
            Dock::Free => "Free (drag it)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Placement {
    pub dock: Dock,
    pub offset: [f32; 2],
}

impl Default for Placement {
    fn default() -> Self {
        Placement { dock: Dock::TopLeft, offset: [8.0, 8.0] }
    }
}

// ---- the settings ------------------------------------------------------------------------------------

/// Limits of the numbers the user can type (see [`DebugSettings::sanitize`]).
pub const SMOOTHING: std::ops::RangeInclusive<f32> = 0.05..=5.0;
pub const HISTORY: std::ops::RangeInclusive<f32> = 5.0..=300.0;
pub const AVERAGE_WINDOW: std::ops::RangeInclusive<f32> = 1.0..=300.0;
pub const TARGET_FPS: std::ops::RangeInclusive<f32> = 10.0..=480.0;
pub const REFRESH: std::ops::RangeInclusive<f32> = 0.05..=2.0;
pub const SPIKE_FACTOR: std::ops::RangeInclusive<f32> = 1.2..=10.0;
pub const OPACITY: std::ops::RangeInclusive<f32> = 0.2..=1.0;
pub const SCALE: std::ops::RangeInclusive<f32> = 0.8..=3.0;
/// The windows start larger than the editor's own text so they can be read over a busy scene.
pub const DEFAULT_SCALE: f32 = 1.45;

/// The frame-rate targets offered as quick choices (and drawn as reference lines).
pub const COMMON_TARGETS: [f32; 4] = [30.0, 60.0, 120.0, 144.0];

/// Which overlays are on and how they behave. The editor saves these with its settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugSettings {
    pub enabled: OverlaySet,
    /// The FPS shown is averaged over this many seconds.
    pub smoothing_s: f32,
    /// How many seconds the graphs show.
    pub history_s: f32,
    /// The window for the average FPS, the 1% low and the spike statistics.
    pub average_window_s: f32,
    /// The frame rate the frame budget and the "below target" warnings refer to.
    pub target_fps: f32,
    /// How often the numbers are refreshed (the graphs refresh at least every 0.1 s).
    pub refresh_s: f32,
    /// A frame is a spike when it takes this many times the typical (median) frame.
    pub spike_factor: f32,
    pub show_instant_fps: bool,
    pub show_target_lines: bool,
    pub highlight_spikes: bool,
    /// Opacity of the panels' background.
    pub opacity: f32,
    /// Size of the panels' text and graphs (1 = the normal text size of the editor).
    pub scale: f32,
    /// Where each overlay's window sits, by overlay id. Only windows that were moved or docked
    /// somewhere else are listed; the others sit in their group's corner (see [`default_dock`]).
    pub panels: BTreeMap<String, Placement>,
    /// Debug shortcuts also work while a text field has the keyboard (off: they would interfere with typing).
    pub shortcuts_while_typing: bool,
}

impl Default for DebugSettings {
    fn default() -> Self {
        DebugSettings {
            enabled: OverlaySet::default(),
            smoothing_s: 0.5,
            history_s: 30.0,
            average_window_s: 10.0,
            target_fps: 60.0,
            refresh_s: 0.25,
            spike_factor: 2.0,
            show_instant_fps: true,
            show_target_lines: true,
            highlight_spikes: true,
            opacity: 0.82,
            scale: DEFAULT_SCALE,
            panels: BTreeMap::new(),
            shortcuts_while_typing: false,
        }
    }
}

impl DebugSettings {
    /// Puts every number back in its range (a hand-edited or older settings file may hold nonsense).
    pub fn sanitize(&mut self) {
        let fix = |v: &mut f32, r: &std::ops::RangeInclusive<f32>, default: f32| {
            *v = if v.is_finite() { v.clamp(*r.start(), *r.end()) } else { default };
        };
        let d = DebugSettings::default();
        fix(&mut self.smoothing_s, &SMOOTHING, d.smoothing_s);
        fix(&mut self.history_s, &HISTORY, d.history_s);
        fix(&mut self.average_window_s, &AVERAGE_WINDOW, d.average_window_s);
        fix(&mut self.target_fps, &TARGET_FPS, d.target_fps);
        fix(&mut self.refresh_s, &REFRESH, d.refresh_s);
        fix(&mut self.spike_factor, &SPIKE_FACTOR, d.spike_factor);
        fix(&mut self.opacity, &OPACITY, d.opacity);
        fix(&mut self.scale, &SCALE, d.scale);
        self.panels.retain(|id, _| Overlay::from_id(id).is_some());
        for p in self.panels.values_mut() {
            for v in &mut p.offset {
                if !v.is_finite() {
                    *v = 8.0;
                }
                *v = v.clamp(-20_000.0, 20_000.0);
            }
        }
    }

    /// Where an overlay's window sits.
    pub fn placement(&self, o: Overlay) -> Placement {
        self.panels.get(o.def().id).copied().unwrap_or_else(|| Placement { dock: default_dock(o.def().group), offset: [8.0, 8.0] })
    }

    /// Moves an overlay's window (a window back in its starting corner is not stored).
    pub fn set_placement(&mut self, o: Overlay, p: Placement) {
        let start = Placement { dock: default_dock(o.def().group), offset: [8.0, 8.0] };
        if p.dock == start.dock && p.dock != Dock::Free {
            self.panels.remove(o.def().id);
        } else {
            self.panels.insert(o.def().id.to_owned(), p);
        }
    }

    /// Docks every window of a group to a corner (or lets them go free where they are).
    pub fn dock_group(&mut self, group: Group, dock: Dock) {
        for def in OVERLAYS.iter().filter(|d| d.group == group) {
            let mut p = self.placement(def.overlay);
            p.dock = dock;
            self.set_placement(def.overlay, p);
        }
    }

    /// The dock the windows of a group share, if they all do.
    pub fn group_dock(&self, group: Group) -> Option<Dock> {
        let mut docks = OVERLAYS.iter().filter(|d| d.group == group).map(|d| self.placement(d.overlay).dock);
        let first = docks.next()?;
        docks.all(|d| d == first).then_some(first)
    }

    /// Puts every window back in its group's corner.
    pub fn reset_layout(&mut self) {
        self.panels.clear();
    }

    /// Settings with the overlays named in a comma-separated list switched on (`fps,fps_graph`, or `all`).
    /// An empty list switches on the FPS and its graph. Unknown names are an error that lists the valid ones.
    pub fn from_list(list: &str) -> Result<DebugSettings, String> {
        let mut settings = DebugSettings::default();
        let names: Vec<&str> = list.split(',').map(str::trim).filter(|n| !n.is_empty()).collect();
        if names.is_empty() {
            settings.enabled.set(Overlay::Fps, true);
            settings.enabled.set(Overlay::FpsGraph, true);
        }
        for name in names {
            if name == "all" {
                OVERLAYS.iter().for_each(|d| settings.enabled.set(d.overlay, true));
            } else {
                let overlay = Overlay::from_id(name).ok_or_else(|| format!("unknown debug overlay '{name}' (known: all, {})", OVERLAYS.iter().map(|d| d.id).collect::<Vec<_>>().join(", ")))?;
                settings.enabled.set(overlay, true);
            }
        }
        Ok(settings)
    }

    /// Restores every setting and switches every overlay off.
    pub fn reset(&mut self) {
        *self = DebugSettings::default();
    }

    pub fn is_default(&self) -> bool {
        *self == DebugSettings::default()
    }

    /// Whether the overlay is on.
    pub fn on(&self, o: Overlay) -> bool {
        self.enabled.contains(o)
    }

    pub fn toggle(&mut self, o: Overlay) -> bool {
        self.enabled.toggle(o)
    }

    pub fn disable_all(&mut self) {
        self.enabled.clear();
    }

    /// The time one frame may take to hit the target frame rate, in milliseconds.
    pub fn budget_ms(&self) -> f32 {
        1000.0 / self.target_fps.max(1.0)
    }

    /// What has to be collected for the overlays that are on.
    pub fn wants(&self) -> Wants {
        self.enabled.iter().fold(Wants::default(), |w, o| w.union(o.wants()))
    }

}

/// The corner a group's windows start in: performance top-left, rendering top-right, scripting and
/// physics bottom-right, diagnostics bottom-left. (Windows of one corner stack on top of each other.)
pub fn default_dock(group: Group) -> Dock {
    match group {
        Group::Performance => Dock::TopLeft,
        Group::Rendering => Dock::TopRight,
        Group::Scripting | Group::Physics => Dock::BottomRight,
        Group::Diagnostics => Dock::BottomLeft,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_lines_up_with_the_variants_and_ids_are_unique() {
        for (i, d) in OVERLAYS.iter().enumerate() {
            assert_eq!(d.overlay as usize, i, "{} is out of order", d.id);
            assert_eq!(d.overlay.def().id, d.id);
            assert_eq!(Overlay::from_id(d.id), Some(d.overlay));
        }
        let mut ids: Vec<_> = OVERLAYS.iter().map(|d| d.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), OVERLAYS.len());
        assert!(OVERLAYS.len() <= 32, "OverlaySet is a 32-bit mask");
        for g in Group::ALL {
            assert!(OVERLAYS.iter().any(|d| d.group == g), "{} has nothing in it", g.label());
        }
    }

    #[test]
    fn only_show_fps_has_a_starting_shortcut() {
        let bound: Vec<_> = OVERLAYS.iter().filter(|d| !d.shortcut.is_empty()).map(|d| (d.overlay, d.shortcut)).collect();
        assert_eq!(bound, vec![(Overlay::Fps, &["Ctrl+Shift+F5"][..])]);
    }

    #[test]
    fn several_overlays_can_be_on_at_once() {
        let mut s = DebugSettings::default();
        assert!(s.toggle(Overlay::Fps) && s.toggle(Overlay::Physics) && s.toggle(Overlay::Warnings));
        assert_eq!(s.enabled.len(), 3);
        assert!(s.on(Overlay::Fps) && s.on(Overlay::Physics) && !s.on(Overlay::Memory));
        assert!(!s.toggle(Overlay::Physics));
        assert_eq!(s.enabled.iter().collect::<Vec<_>>(), vec![Overlay::Fps, Overlay::Warnings]);
        s.disable_all();
        assert!(s.enabled.is_empty());
    }

    #[test]
    fn collection_is_only_switched_on_for_what_is_shown() {
        let mut s = DebugSettings::default();
        assert_eq!(s.wants(), Wants::default(), "nothing shown, nothing collected");
        s.toggle(Overlay::Fps);
        assert_eq!(s.wants(), Wants { frames: true, ..Default::default() }, "the FPS only needs the frame times, which are always recorded");
        s.toggle(Overlay::ScriptPerformance);
        assert!(s.wants().script_costs && s.wants().scripts && !s.wants().scene && !s.wants().graph);
        s.toggle(Overlay::FpsGraph);
        assert!(s.wants().graph);
        s.toggle(Overlay::Triangles);
        assert!(s.wants().scene && !s.wants().gpu);
        s.toggle(Overlay::RenderStats);
        assert!(s.wants().gpu);
    }

    #[test]
    fn settings_round_trip_and_unknown_things_are_ignored() {
        let mut s = DebugSettings::default();
        s.toggle(Overlay::FpsGraph);
        s.toggle(Overlay::Memory);
        s.target_fps = 144.0;
        s.set_placement(Overlay::Memory, Placement { dock: Dock::Free, offset: [120.0, 40.0] });
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.contains("\"fps_graph\"") && text.contains("\"memory\""), "{text}");
        let back: DebugSettings = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);

        let odd: DebugSettings = serde_json::from_str(r#"{ "enabled": ["fps", "from_the_future"], "target_fps": 90, "from_the_future": 1 }"#).unwrap();
        assert_eq!(odd.enabled.iter().collect::<Vec<_>>(), vec![Overlay::Fps]);
        assert_eq!(odd.target_fps, 90.0);
        assert_eq!(odd.history_s, DebugSettings::default().history_s, "missing fields take their defaults");
    }

    #[test]
    fn a_command_line_list_switches_overlays_on() {
        let s = DebugSettings::from_list("fps, physics ,memory").unwrap();
        // (They come back in the order the overlays are declared, not the order they were asked for.)
        let on: Vec<Overlay> = s.enabled.iter().collect();
        assert_eq!(on.len(), 3);
        assert!([Overlay::Fps, Overlay::Memory, Overlay::Physics].iter().all(|o| on.contains(o)), "{on:?}");
        assert_eq!(DebugSettings::from_list("").unwrap().enabled.iter().collect::<Vec<_>>(), vec![Overlay::Fps, Overlay::FpsGraph]);
        assert_eq!(DebugSettings::from_list("all").unwrap().enabled.len(), OVERLAYS.len());
        let err = DebugSettings::from_list("fps,nonsense").unwrap_err();
        assert!(err.contains("nonsense") && err.contains("frame_time_graph"), "{err}");
    }

    #[test]
    fn numbers_are_kept_in_range() {
        let mut s: DebugSettings = serde_json::from_str(r#"{ "smoothing_s": -3, "history_s": 1e9, "target_fps": 0, "opacity": 7, "refresh_s": 0 }"#).unwrap();
        s.sanitize();
        assert_eq!((s.smoothing_s, s.history_s, s.target_fps, s.opacity, s.refresh_s), (0.05, 300.0, 10.0, 1.0, 0.05));
        s.target_fps = f32::NAN;
        s.scale = 99.0;
        s.set_placement(Overlay::Fps, Placement { dock: Dock::Free, offset: [f32::INFINITY, 5.0] });
        s.panels.insert("from_the_future".into(), Placement::default());
        s.sanitize();
        assert_eq!((s.target_fps, s.scale), (60.0, 3.0));
        assert!(s.placement(Overlay::Fps).offset[0].is_finite());
        assert!(!s.panels.contains_key("from_the_future"), "windows of overlays this version does not have are dropped");
        assert!((s.budget_ms() - 16.666).abs() < 0.01);
    }

    #[test]
    fn every_overlay_has_its_own_window_that_starts_in_its_groups_corner() {
        let mut s = DebugSettings::default();
        for d in OVERLAYS {
            assert_eq!(s.placement(d.overlay).dock, default_dock(d.group), "{}", d.name);
        }
        assert!(s.panels.is_empty(), "nothing is stored until a window is moved");
        // Moving one window changes only that one.
        s.set_placement(Overlay::Fps, Placement { dock: Dock::BottomLeft, offset: [8.0, 8.0] });
        assert_eq!(s.placement(Overlay::Fps).dock, Dock::BottomLeft);
        assert_eq!(s.placement(Overlay::FpsGraph).dock, Dock::TopLeft);
        assert_eq!(s.group_dock(Group::Performance), None, "the group is split up now");
        // Putting it back in the corner it started in stores nothing.
        s.set_placement(Overlay::Fps, Placement { dock: Dock::TopLeft, offset: [8.0, 8.0] });
        assert!(s.panels.is_empty());
        // A whole group at once.
        s.dock_group(Group::Rendering, Dock::BottomLeft);
        assert_eq!(s.group_dock(Group::Rendering), Some(Dock::BottomLeft));
        assert_eq!(s.placement(Overlay::Triangles).dock, Dock::BottomLeft);
        assert_eq!(s.placement(Overlay::Memory).dock, Dock::BottomLeft, "diagnostics start there anyway");
        s.reset_layout();
        assert_eq!(s.group_dock(Group::Rendering), Some(Dock::TopRight));
    }
}
