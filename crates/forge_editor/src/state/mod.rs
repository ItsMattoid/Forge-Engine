//! Editor state. The data model is the single source of truth; the Bevy entities
//! in the viewport are a mirror kept up to date by the renderer.
//!
//! The [`Editor`] struct lives here; its operations are grouped by topic:
//!
//! * [`history`]     - snapshot undo / redo
//! * [`project_ops`] - projects, scenes, scripts, saving
//! * [`tree_edit`]   - inserting, deleting, duplicating, re-parenting instances
//! * [`tree_view`]   - what the Explorer lists (services, filter) and how clicking its rows selects
//! * [`session`]     - playing in a separate window, validating and building the game
//! * [`play`]        - playing inside the viewport, with live editing

mod actions;
mod assets;
mod audio;
mod history;
mod layout;
mod materials;
mod pick;
mod play;
mod project_ops;
mod selection;
mod session;
mod tree_edit;
mod tree_view;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Child,
    rc::Rc,
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
    },
};

use bevy::math::Vec3;
use bevy_egui::egui::Modifiers;
use forge_engine::{
    assets::GameSource,
    dm::{journal::Detached, DataModel, Id},
    project::Project,
};

use crate::{
    dock::Dock,
    keymap::{self, Action, Context},
    settings::EditorSettings,
};

pub use assets::{AssetBrowser, AssetDrag, PendingDrop, Thumb};
#[cfg(test)]
pub use assets::AssetPlan;
pub use audio::EditorAudio;
pub use materials::MaterialEditor;
pub use pick::{AssetPicker, PickTarget};
pub use history::History;
pub use layout::PanelOp;
pub use selection::{PickCycle, SelectionState};
pub use play::PlaySession;
pub use tree_edit::DuplicateRecord;
pub use tree_view::{classes_in_scene, TreeFilter, TreeRow};

/// What a click-drag in the viewport does to the selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tool {
    /// Only picks objects.
    #[default]
    Select,
    Move,
    Scale,
    Rotate,
    /// Edits where the selected object turns and moves from (its `PivotOffset`), not the object itself.
    Pivot,
}

impl Tool {
    pub const ALL: [Tool; 5] = [Tool::Select, Tool::Move, Tool::Scale, Tool::Rotate, Tool::Pivot];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Move => "Move",
            Tool::Scale => "Scale",
            Tool::Rotate => "Rotate",
            Tool::Pivot => "Pivot",
        }
    }

    /// The shortcut that switches to this tool (shown in tooltips).
    pub fn action(self) -> Action {
        match self {
            Tool::Select => Action::ToolSelect,
            Tool::Move => Action::ToolMove,
            Tool::Scale => Action::ToolScale,
            Tool::Rotate => Action::ToolRotate,
            Tool::Pivot => Action::ToolPivot,
        }
    }
}

/// Which of the two fly speeds the viewport is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpeedKind {
    /// The main fly speed.
    Fly,
    /// The second-speed multiplier.
    Second,
}

/// Which snapping value a small popup edits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Increment {
    Move,
    Rotate,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsTab {
    Shortcuts,
    Camera,
    Snapping,
    Selection,
    /// V-Sync, the frame-rate limit and anti-aliasing.
    Rendering,
    /// The Debug menu's overlays: frame-rate targets, history, panel positions.
    Debug,
    /// The colours of the editor.
    Colors,
}

impl SettingsTab {
    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Shortcuts => "Shortcuts",
            SettingsTab::Camera => "Camera",
            SettingsTab::Snapping => "Snapping",
            SettingsTab::Selection => "Selection",
            SettingsTab::Rendering => "Rendering",
            SettingsTab::Debug => "Debug Overlays",
            SettingsTab::Colors => "Colors",
        }
    }
}

/// The tabs of the settings window, in groups (each group is a dropdown in the list on the left), in the Common
/// order: what people open first (how they work: the keys, then the viewport) before how it looks and the diagnostics.
pub const SETTINGS_TABS: &[(&str, &[SettingsTab])] = &[
    ("Input", &[SettingsTab::Shortcuts]),
    ("Viewport", &[SettingsTab::Camera, SettingsTab::Selection, SettingsTab::Snapping, SettingsTab::Rendering]),
    ("Interface", &[SettingsTab::Colors]),
    ("Debug", &[SettingsTab::Debug]),
];

/// The tab groups in the order chosen in the Shortcuts tab: as listed above (Common), or the groups and the tabs
/// in them by the names shown (Alphabetical).
pub fn settings_tabs(order: crate::settings::SettingsOrder) -> Vec<(&'static str, Vec<SettingsTab>)> {
    let mut groups: Vec<(&'static str, Vec<SettingsTab>)> = SETTINGS_TABS.iter().map(|(g, tabs)| (*g, tabs.to_vec())).collect();
    if order == crate::settings::SettingsOrder::Alphabetical {
        for (_, tabs) in &mut groups {
            tabs.sort_by_key(|t| t.label().to_lowercase());
        }
        groups.sort_by_key(|(g, _)| g.to_lowercase());
    }
    groups
}

/// The tabs of the Project Settings window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjectTab {
    General,
    /// Single-player or multiplayer.
    GameType,
    /// The game window: title, size, display mode.
    Display,
    /// Quality, shadows, V-Sync, frame-rate limit.
    Graphics,
    /// The icon and the splash screen.
    Launch,
    /// What only a multiplayer game has (shown only for one).
    Multiplayer,
}

impl ProjectTab {
    pub fn label(self) -> &'static str {
        match self {
            ProjectTab::General => "General",
            ProjectTab::GameType => "Game Type",
            ProjectTab::Display => "Display",
            ProjectTab::Graphics => "Graphics",
            ProjectTab::Launch => "Icon & Splash Screen",
            ProjectTab::Multiplayer => "Multiplayer",
        }
    }
}

/// The tabs of the Project Settings window, in groups (like the Editor Settings window): the project itself, then
/// how the game presents itself, then what is specific to the game type.
pub const PROJECT_TABS: &[(&str, &[ProjectTab])] = &[
    ("Project", &[ProjectTab::General, ProjectTab::GameType]),
    ("Game Window", &[ProjectTab::Display, ProjectTab::Graphics, ProjectTab::Launch]),
    ("Multiplayer", &[ProjectTab::Multiplayer]),
];

/// The tab groups that apply to a game of this type: the Multiplayer page is only there for a multiplayer game.
pub fn project_tabs(game_type: forge_engine::project::GameType) -> Vec<(&'static str, &'static [ProjectTab])> {
    PROJECT_TABS.iter().filter(|(_, tabs)| game_type.is_multiplayer() || !tabs.contains(&ProjectTab::Multiplayer)).copied().collect()
}

/// A picture chosen as the game's icon or splash screen, as the Project Settings window last looked at it: what was
/// found out about the file and its small preview, kept until the file changes.
pub struct LaunchPreview {
    /// The file's size and time when it was looked at.
    pub stamp: (u64, Option<std::time::SystemTime>),
    /// Whether it is usable for its job (see `forge_engine::project::launch`), and if not, why not.
    pub result: Result<forge_engine::project::launch::ImageInfo, String>,
    pub texture: Option<bevy_egui::egui::TextureHandle>,
}

/// State of the Project Settings window (a window of its own, not part of the dock).
pub struct ProjectSettingsUi {
    pub open: bool,
    /// The window has been put where it was last time (done on the first frame after it opens).
    pub placed: bool,
    pub tab: ProjectTab,
    /// Something changed that is not written to `Project.toml` yet (written when the mouse and keyboard are idle, and on close).
    pub pending_save: bool,
    /// What the Icon & Splash Screen page knows about the chosen pictures (by role and path).
    pub launch_previews: HashMap<String, LaunchPreview>,
    /// Why the picture last chosen for the icon or the splash screen was not accepted.
    pub launch_message: Option<(forge_engine::project::launch::Role, String)>,
}

impl Default for ProjectSettingsUi {
    fn default() -> Self {
        ProjectSettingsUi { open: false, placed: false, tab: ProjectTab::General, pending_save: false, launch_previews: HashMap::new(), launch_message: None }
    }
}

/// A shortcut waiting for a key to be pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capturing {
    pub action: Action,
    /// The key being replaced (`None` = a key is being added).
    pub replacing: Option<keymap::Chord>,
}

/// State of the editor settings window (a window of its own, not part of the dock).
pub struct SettingsUi {
    pub open: bool,
    /// The window has been put where it was last time (done on the first frame after it opens).
    pub placed: bool,
    pub tab: SettingsTab,
    /// The Shortcuts tab's search box.
    pub search: String,
    /// The shortcut waiting for its new key to be pressed.
    pub capturing: Option<Capturing>,
    pub capture_mods: Modifiers,
    /// Why the last key was refused.
    pub message: Option<String>,
}

impl SettingsUi {
    /// Starts waiting for a key for `action`: a new one, or the replacement of `replacing`.
    pub fn begin_capture(&mut self, action: Action, replacing: Option<keymap::Chord>) {
        self.capturing = Some(Capturing { action, replacing });
        self.capture_mods = Modifiers::default();
        self.message = None;
    }
}

impl Default for SettingsUi {
    fn default() -> Self {
        SettingsUi { open: false, placed: false, tab: SettingsTab::Shortcuts, search: String::new(), capturing: None, capture_mods: Modifiers::default(), message: None }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

pub struct LogLine {
    pub level: Level,
    pub text: String,
}

pub enum Dialog {
    None,
    NewProject { name: String, dir: String },
    Build { out_dir: String },
    NewScene { name: String },
    /// Deleting files from the Assets tab, with what would be left pointing at them.
    DeleteAssets { paths: Vec<String>, used_by: Vec<String> },
}

pub struct OpenScript {
    pub path: String,
    pub text: String,
    pub modified: bool,
    /// The file's size and time as this editor last read or wrote it.
    pub stamp: Option<(u64, Option<std::time::SystemTime>)>,
    /// The file as the last scan saw it (to tell a change by someone else from our own save).
    pub seen: Option<(u64, Option<std::time::SystemTime>)>,
    /// The file changed on disk while there are unsaved changes here.
    pub conflict: bool,
    /// The file is gone from disk.
    pub missing: bool,
}

/// Not a Bevy `Resource`: it holds `Rc`s, so systems access it as `NonSend`.
pub struct Editor {
    pub project: Option<Project>,
    pub scene_path: Option<String>,
    pub scene_name: String,
    pub dm: Rc<RefCell<DataModel>>,
    /// The active object: the last one picked (see `state::selection`).
    pub selected: Option<Id>,
    /// Every selected object, the active one included.
    pub selection: Vec<Id>,
    /// What the Properties panel shows when nothing is selected: the last object that was.
    pub last_inspected: Option<Id>,
    /// Alt-click cycling through objects behind each other.
    pub pick_cycle: PickCycle,
    /// Which fly speed was just changed with the wheel, and until when to show it (see `viewport`).
    pub speed_hud: Option<(SpeedKind, std::time::Instant)>,
    /// The selection as last recorded in the undo history (see `history`).
    pub sel_committed: SelectionState,
    pub history: History,
    pub dirty: bool,
    pub focus_request: bool,
    /// Expand the explorer to the selection and scroll to it.
    pub reveal: bool,
    /// Editor camera position and forward direction (new parts are placed where it looks).
    pub cam_ray: (Vec3, Vec3),
    pub log: Vec<LogLine>,
    pub script: Option<OpenScript>,
    pub dialog: Dialog,
    /// The Explorer rows that are closed (by instance uid).
    pub collapsed: HashSet<u32>,
    /// The Explorer's search box and its settings (see `tree_view`).
    pub tree_filter: TreeFilter,
    /// Where a Shift-click range in the Explorer starts: the row last clicked without Shift.
    pub anchor: Option<Id>,
    pub scenes: Vec<String>,
    pub scripts: Vec<String>,
    /// The Asset Browser (see `state::assets`).
    pub assets_ui: AssetBrowser,
    /// Assets dropped on the viewport, waiting for the viewport to find what is under the pointer.
    pub pending_drop: Option<PendingDrop>,
    /// Script objects that were renamed, with their old name and when: their files follow (see `queue_script_rename`).
    pub script_renames: Vec<(Id, String, std::time::Instant)>,
    /// The objects whose Parent is being chosen: the next click on an object in the Explorer or the viewport
    /// makes that object their parent (see `pick_parent`).
    pub picking_parent: Option<Vec<Id>>,
    /// The model whose `PrimaryPart` is being chosen: the next click on a part inside it is the choice (see `pick_primary_part`).
    pub picking_primary: Option<Id>,
    /// Set when a mesh file was reimported: the viewport forgets its loaded meshes.
    pub reload_meshes: std::cell::Cell<bool>,
    /// Files in the project's `assets/` folder (for asset-picking properties).
    pub assets: Vec<String>,
    /// Where the viewport loads project files (meshes) from.
    pub source: GameSource,
    /// The window that chooses an asset for a property or a material slot (see state::pick).
    pub picker: Option<AssetPicker>,
    /// The Material Editor window (see state::materials).
    pub material_editor: MaterialEditor,
    /// Sounds: the preview in the Asset Browser, and the mixer while a game plays (see state::audio).
    pub audio: EditorAudio,
    /// Files this editor wrote itself and expects the next scan to report as changed (so they are not mistaken for outside edits).
    pub own_writes: HashSet<String>,
    /// The editor's own settings (shortcuts, camera speed, snapping), stored per user.
    pub settings: EditorSettings,
    pub settings_ui: SettingsUi,
    pub project_settings_ui: ProjectSettingsUi,
    pub tool: Tool,
    /// The window the mouse is over: shortcuts only fire in the window they belong to.
    pub hover: Context,
    /// What the keyboard asked for this frame (held camera keys, the wheel shortcut).
    pub keys: keymap::Frame,
    /// Screen areas of the Explorer and Properties panels, as drawn last frame.
    pub explorer_rect: Option<bevy_egui::egui::Rect>,
    pub properties_rect: Option<bevy_egui::egui::Rect>,
    /// The editor's windows: where each panel is, which are shown (see `dock` and `state::layout`).
    pub dock: Dock,
    /// The dock is out of the editor while it is being drawn: requests to change it wait in `panel_ops`.
    pub dock_busy: bool,
    pub panel_ops: Vec<PanelOp>,
    /// The layout as last written to the editor settings (to see when it changed).
    pub layout_saved: String,
    /// The panel the mouse is over, as the keyboard shortcuts see it (the viewport is decided separately).
    pub panel_hover: Option<Context>,
    /// The mouse is over the viewport's picture (not a window or panel on top of it).
    pub viewport_over: bool,
    /// The camera is being flown or turned: started over the viewport, it goes on whatever the mouse
    /// passes over until the keys and the mouse button are let go (see `ui::navigation_latch`).
    pub nav_latched: bool,
    /// A handle or an object is being dragged in the viewport (set by the viewport at the end of every frame):
    /// while it is, the drag owns the objects' transforms, so a quarter-turn shortcut is handed to it instead
    /// of changing them behind its back (see `pending_turns`).
    pub drag_active: bool,
    /// Quarter turns (the axis, 0 = X, 1 = Y) asked for while a drag was going on; the viewport applies them
    /// to the drag itself, so it carries on from the turned objects instead of putting them back.
    pub pending_turns: Vec<usize>,
    /// The last duplication, for "Repeat Duplicate" (see `state::tree_edit`).
    pub last_duplicate: Option<DuplicateRecord>,
    /// What Copy / Cut took, ready to Paste.
    pub clipboard: Vec<Detached>,
    /// The object being renamed in the Explorer, with the text typed so far.
    pub renaming: Option<(Id, String)>,
    /// The "insert object" popup (its search text), opened over the viewport.
    pub quick_insert: Option<String>,
    /// The small popup for a snapping value.
    pub increment_popup: Option<Increment>,
    /// The user asked to forget the collected debug data (frame history, peaks, script totals).
    pub clear_debug_data: bool,
    /// The game running inside the viewport (see `state::play`).
    pub play: Option<PlaySession>,
    /// The game running in its own window (a separate process).
    pub child: Option<Child>,
    tx: Sender<String>,
    rx: Mutex<Receiver<String>>,
}

impl Editor {
    pub fn new(initial: Option<PathBuf>) -> Self {
        let (settings, settings_problem) = EditorSettings::load();
        let (tx, rx) = mpsc::channel();
        let dock = Self::initial_dock(settings.layout.as_ref());
        let layout_saved = dock.to_json().to_string();
        let mut ed = Self {
            project: None,
            scene_path: None,
            scene_name: "Main".into(),
            dm: Rc::new(RefCell::new(DataModel::new())),
            selected: None,
            selection: Vec::new(),
            last_inspected: None,
            pick_cycle: PickCycle::default(),
            speed_hud: None,
            sel_committed: SelectionState::default(),
            history: History::default(),
            dirty: false,
            focus_request: false,
            reveal: false,
            cam_ray: (Vec3::new(0.0, 10.0, 20.0), Vec3::NEG_Z),
            log: Vec::new(),
            script: None,
            dialog: Dialog::None,
            collapsed: HashSet::new(),
            tree_filter: TreeFilter::default(),
            anchor: None,
            scenes: Vec::new(),
            scripts: Vec::new(),
            assets_ui: AssetBrowser::default(),
            pending_drop: None,
            reload_meshes: std::cell::Cell::new(false),
            script_renames: Vec::new(),
            picking_parent: None,
            picking_primary: None,
            assets: Vec::new(),
            source: GameSource::Dir(PathBuf::new()),
            picker: None,
            material_editor: MaterialEditor::default(),
            audio: EditorAudio::default(),
            own_writes: HashSet::new(),
            settings,
            settings_ui: SettingsUi::default(),
            project_settings_ui: ProjectSettingsUi::default(),
            tool: Tool::default(),
            hover: Context::Global,
            keys: keymap::Frame::default(),
            explorer_rect: None,
            properties_rect: None,
            dock,
            dock_busy: false,
            panel_ops: Vec::new(),
            layout_saved,
            panel_hover: None,
            viewport_over: false,
            nav_latched: false,
            drag_active: false,
            pending_turns: Vec::new(),
            last_duplicate: None,
            clipboard: Vec::new(),
            renaming: None,
            quick_insert: None,
            increment_popup: None,
            clear_debug_data: false,
            play: None,
            child: None,
            tx,
            rx: Mutex::new(rx),
        };
        // Every edit made through the editor is recorded so it can be undone (see `history`).
        ed.dm.borrow_mut().start_journal();
        ed.info("Forge editor ready. Create or open a project from the File menu.");
        if let Some(problem) = settings_problem {
            ed.warn(problem);
        }
        for note in std::mem::take(&mut ed.settings.startup_notes) {
            ed.warn(note);
        }
        if let Some(path) = initial {
            ed.open_project(&path);
        }
        ed
    }

    // ---- logging ----------------------------------------------------------------

    pub fn push_log(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
        // The Warnings & Errors overlay lists these too. (Output of the running game is already
        // recorded where scripts print it, so it is not counted a second time.)
        if !text.starts_with("[game]") {
            let severity = match level {
                Level::Warn => Some(forge_engine::diag::log::Severity::Warning),
                Level::Error => Some(forge_engine::diag::log::Severity::Error),
                Level::Info => None,
            };
            if let Some(severity) = severity {
                forge_engine::diag::log::push(severity, "editor", &text);
            }
        }
        self.log.push(LogLine { level, text });
        if self.log.len() > 5000 {
            self.log.drain(..1000);
        }
    }

    pub fn info(&mut self, t: impl Into<String>) {
        self.push_log(Level::Info, t);
    }

    pub fn warn(&mut self, t: impl Into<String>) {
        self.push_log(Level::Warn, t);
    }

    pub fn error(&mut self, t: impl Into<String>) {
        self.push_log(Level::Error, t);
    }

    /// Pulls output from the running game and detects when it exits.
    pub fn poll(&mut self) {
        let lines: Vec<String> = self.rx.lock().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for l in lines {
            let level = if l.contains(" ERROR ") {
                Level::Error
            } else if l.contains(" WARN ") {
                Level::Warn
            } else {
                Level::Info
            };
            self.push_log(level, format!("[game] {l}"));
        }
        let status = self.child.as_mut().and_then(|c| c.try_wait().ok().flatten());
        if let Some(status) = status {
            self.info(format!("Game exited ({status})."));
            self.child = None;
        }
    }
}
