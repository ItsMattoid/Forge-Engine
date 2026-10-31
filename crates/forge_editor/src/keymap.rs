//! Keyboard shortcuts: what can be bound, the starter bindings, and reading them each frame.
//!
//! Every shortcut is an [`Action`] described once in [`DEFS`] (name, category, the window it works
//! in, default binding). The Shortcuts tab of the settings window is generated from that table, so
//! adding an action means adding one line there and handling it where actions are applied.
//!
//! Shortcuts are *contextual*: an action only fires while the mouse is over the window it belongs
//! to (the viewport, the Explorer, the Properties panel); `General` actions work over any window.
//! Two actions may share a chord as long as their windows differ (Ctrl+R rotates in the viewport
//! and renames in the Explorer); the window the mouse is over decides which one fires.

use std::collections::{BTreeMap, HashSet};

use bevy_egui::egui::{self, Key, Modifiers};
use forge_engine::diag::{Overlay, OVERLAYS};

use crate::dock::{Panel, PANELS};

// ---- contexts and kinds ---------------------------------------------------------------------

/// The window a shortcut works in.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Context {
    /// Any window.
    Global,
    Viewport,
    Explorer,
    Properties,
}

impl Context {
    pub fn label(self) -> &'static str {
        match self {
            Context::Global => "Anywhere",
            Context::Viewport => "Viewport",
            Context::Explorer => "Explorer",
            Context::Properties => "Properties",
        }
    }

    /// Whether a shortcut of this context can be active while the mouse is over `hovered`.
    pub fn active_over(self, hovered: Context) -> bool {
        self == Context::Global || self == hovered
    }

    /// Whether two shortcuts could both be active at once (so they must not share a chord).
    pub fn overlaps(self, other: Context) -> bool {
        self == other || self == Context::Global || other == Context::Global
    }
}

/// How a shortcut is triggered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Fires once when the chord is pressed.
    Press,
    /// Active for as long as the chord is held.
    Hold,
    /// Fires when the mouse wheel turns while the modifiers are held.
    Wheel,
}

// ---- actions --------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    // Viewport
    CamForward,
    CamLeft,
    CamBack,
    CamRight,
    CamUp,
    CamDown,
    SnapTo,
    FastCamera,
    FastSpeedWheel,
    PrimarySpeedWheel,
    ZoomWheel,
    InsertObject,
    Wireframe,
    WireframeOnly,
    // Tools
    ToolSelect,
    ToolMove,
    ToolScale,
    ToolRotate,
    ToolPivot,
    TransformSpace,
    Rotate90Y,
    Rotate90X,
    MoveIncrement,
    RotateIncrement,
    // Scene tree
    Group,
    Folder,
    Ungroup,
    FindSelected,
    Rename,
    // General
    Save,
    Duplicate,
    RepeatDuplicate,
    Copy,
    Paste,
    Cut,
    Undo,
    Redo,
    Delete,
    Play,
    Stop,
    EditorSettings,
    ProjectSettings,
    // Windows: one shortcut per dockable panel (generated from `dock::PANELS`) and one for the default layout.
    Window(Panel),
    RestoreLayout,
    // Debug: one shortcut per overlay of the Debug menu (generated from `forge_engine::diag::OVERLAYS`).
    Debug(Overlay),
}

/// Everything about one shortcut except the keys the user chose.
pub struct ActionDef {
    pub action: Action,
    /// Stable name used in the settings file.
    pub id: &'static str,
    pub name: &'static str,
    /// Group in the settings window.
    pub category: &'static str,
    pub context: Context,
    pub kind: Kind,
    /// The starter keys (see [`Chord::parse`]); a shortcut can have several.
    pub defaults: &'static [&'static str],
    /// What the shortcut does, shown in the settings window.
    pub description: &'static str,
    /// Still fires while a text field has the keyboard (saving, playing, opening windows).
    pub while_typing: bool,
}

const fn def(action: Action, id: &'static str, name: &'static str, category: &'static str, context: Context, kind: Kind, defaults: &'static [&'static str], description: &'static str) -> ActionDef {
    ActionDef { action, id, name, category, context, kind, defaults, description, while_typing: false }
}

const fn typing(mut d: ActionDef) -> ActionDef {
    d.while_typing = true;
    d
}

/// The shortcut that toggles an overlay: its name, description and starting keys come from the
/// overlay's own row in `forge_engine::diag::OVERLAYS`. Works over any window.
const fn debug(overlay: Overlay) -> ActionDef {
    let o = &OVERLAYS[overlay as usize];
    ActionDef { action: Action::Debug(overlay), id: o.key, name: o.name, category: DEBUG, context: Context::Global, kind: Kind::Press, defaults: o.shortcut, description: o.description, while_typing: false }
}

/// The shortcut that shows, focuses or hides a panel: shown -> in front -> hidden. Its name, id and
/// starting keys come from the panel's own row in `dock::PANELS`. Works over any window, but not while
/// typing (it must not interfere with text fields or the script editor).
const fn window(panel: Panel) -> ActionDef {
    let p = &PANELS[panel as usize];
    ActionDef { action: Action::Window(panel), id: p.action_id, name: p.action_name, category: WINDOWS, context: Context::Global, kind: Kind::Press, defaults: p.shortcut, description: "Show the window; if it is already shown but not in use, bring it to the front; if it is the one in use, hide it.", while_typing: false }
}

use Action as A;
use Context::{Explorer as Tree, Global as Any, Viewport as View};
use Kind::{Hold, Press, Wheel};

const VIEWPORT: &str = "Viewport";
const TOOLS: &str = "Tools";
const SCENE_TREE: &str = "Scene Tree";
const GENERAL: &str = "General";
const DEBUG: &str = "Debug";
const WINDOWS: &str = "Windows";

/// Every shortcut, in the order the settings window lists them.
pub static DEFS: &[ActionDef] = &[
    // General
    typing(def(A::Save, "general.save", "Save project", GENERAL, Any, Press, &["Ctrl+S"], "Write the scene, project settings and open script to disk.")),
    def(A::Undo, "general.undo", "Undo", GENERAL, Any, Press, &["Ctrl+Z"], "Undo the last edit."),
    def(A::Redo, "general.redo", "Redo", GENERAL, Any, Press, &["Ctrl+Shift+Z", "Ctrl+Y"], "Redo what was undone."),
    def(A::Cut, "general.cut", "Cut", GENERAL, Any, Press, &["Ctrl+X"], "Copy the selected objects and delete them."),
    def(A::Copy, "general.copy", "Copy", GENERAL, Any, Press, &["Ctrl+C"], "Copy the selected objects."),
    def(A::Paste, "general.paste", "Paste", GENERAL, Any, Press, &["Ctrl+V"], "Paste what was copied into the selected container, or the workspace."),
    def(A::Duplicate, "general.duplicate", "Duplicate", GENERAL, Any, Press, &["Ctrl+D"], "Make a copy of each selected object next to it."),
    def(A::RepeatDuplicate, "general.repeat_duplicate", "Repeat duplicate", GENERAL, Any, Press, &["Ctrl+Shift+D"], "Duplicate the last duplicate again, moved and turned by the same amount (Duplicate, move the copy, then press this again and again for evenly spaced copies). Needs the copies to be selected; undo, Play and opening another scene reset it."),
    def(A::Delete, "general.delete", "Delete", GENERAL, Any, Press, &["Delete"], "Delete the selected objects."),
    typing(def(A::Play, "general.play", "Play / Stop", GENERAL, Any, Press, &["F5"], "Run the game in the viewport, or stop it if it is running.")),
    typing(def(A::Stop, "general.stop", "Stop", GENERAL, Any, Press, &["Shift+F5"], "Stop the running game and put the scene back.")),
    typing(def(A::EditorSettings, "general.editor_settings", "Editor settings", GENERAL, Any, Press, &["Alt+S"], "Open or close the editor settings window.")),
    typing(def(A::ProjectSettings, "general.project_settings", "Project settings", GENERAL, Any, Press, &["Ctrl+Alt+S"], "Open the project's settings.")),
    def(A::CamForward, "viewport.forward", "Camera forward", VIEWPORT, View, Hold, &["W"], "Fly the camera forward while the mouse is over the viewport."),
    def(A::CamLeft, "viewport.left", "Camera left", VIEWPORT, View, Hold, &["A"], "Fly the camera to the left."),
    def(A::CamBack, "viewport.backward", "Camera backward", VIEWPORT, View, Hold, &["S"], "Fly the camera backward."),
    def(A::CamRight, "viewport.right", "Camera right", VIEWPORT, View, Hold, &["D"], "Fly the camera to the right."),
    def(A::CamUp, "viewport.up", "Camera up", VIEWPORT, View, Hold, &["E"], "Fly the camera straight up."),
    def(A::CamDown, "viewport.down", "Camera down", VIEWPORT, View, Hold, &["Q"], "Fly the camera straight down."),
    def(A::SnapTo, "viewport.snap_to", "Focus selection", VIEWPORT, View, Press, &["F"], "Frame the selected object and zoom around it from then on (flying with the move keys ends that). The only shortcut that moves the camera to the selection."),
    def(A::FastCamera, "viewport.fast", "Secondary camera speed", VIEWPORT, View, Hold, &["Shift"], "Hold to fly at the secondary speed (a multiplier of the fly speed; below 1x makes it a slow, precise mode)."),
    def(A::FastSpeedWheel, "viewport.fast_speed", "Adjust Secondary Camera Speed", VIEWPORT, View, Wheel, &["Shift+Scroll"], "Hold the key and turn the wheel to raise or lower the secondary camera speed (the multiplier the secondary speed key applies to the fly speed)."),
    def(A::PrimarySpeedWheel, "viewport.fly_speed", "Adjust Fly Camera Speed", VIEWPORT, View, Wheel, &["Alt+Scroll"], "Hold the key and turn the wheel to raise or lower the main fly camera speed. Works while flying, and the movement keys keep working while it is held."),
    def(A::ZoomWheel, "viewport.zoom", "Zoom", VIEWPORT, View, Wheel, &["Scroll"], "Move the camera forward or back along the view (see Camera > Zoom behaviour in the editor settings). Selecting something never changes where it zooms."),
    def(A::InsertObject, "viewport.insert", "Insert object", VIEWPORT, View, Press, &["Shift+Space"], "Open the search to insert a new object."),
    def(A::Wireframe, "viewport.wireframe", "Wireframe", VIEWPORT, View, Press, &["Z"], "Draw the edges of every mesh over the scene (needs a graphics API that can: Vulkan, DirectX 12 or Metal)."),
    def(A::WireframeOnly, "viewport.wireframe_only", "Wireframe without solids", VIEWPORT, View, Press, &["Shift+Z"], "While the wireframe is on, hide the shaded parts and draw only the edges."),
    // Tools (they act on what is in the viewport)
    def(A::ToolSelect, "tool.select", "Select tool", TOOLS, View, Press, &["1"], "Only select objects: no handles."),
    def(A::ToolMove, "tool.move", "Move tool", TOOLS, View, Press, &["2"], "Move the selection with arrows (one axis) and planes (two axes)."),
    def(A::ToolScale, "tool.scale", "Scale tool", TOOLS, View, Press, &["3"], "Resize the selection by dragging a sphere on one of its sides."),
    def(A::ToolRotate, "tool.rotate", "Rotate tool", TOOLS, View, Press, &["4"], "Rotate the selection around an axis, or freely with the trackball in the middle."),
    def(A::ToolPivot, "tool.pivot", "Pivot tool", TOOLS, View, Press, &["5"], "Move the selected object's pivot (where it turns and moves from) without moving the object."),
    def(A::TransformSpace, "tool.space", "World / local space", TOOLS, View, Press, &["Ctrl+Backtick"], "Switch the move and rotate handles between the world's axes and the object's own."),
    def(A::Rotate90Y, "tool.rotate_y", "Rotate 90 degrees around Y", TOOLS, View, Press, &["Ctrl+R"], "Turn the selection a quarter around the Y axis."),
    def(A::Rotate90X, "tool.rotate_x", "Rotate 90 degrees around X", TOOLS, View, Press, &["Ctrl+T"], "Turn the selection a quarter around the X axis."),
    def(A::MoveIncrement, "tool.move_increment", "Move increment", TOOLS, View, Press, &["Shift+2"], "Edit the grid the move and scale tools snap to."),
    def(A::RotateIncrement, "tool.rotate_increment", "Rotate increment", TOOLS, View, Press, &["Shift+4"], "Edit the step the rotate tool snaps to."),
    // Scene tree
    def(A::Group, "tree.group", "Group", SCENE_TREE, Tree, Press, &["Ctrl+G"], "Put the selected objects in a new Model."),
    def(A::Folder, "tree.folder", "Folder", SCENE_TREE, Tree, Press, &["Ctrl+Shift+G"], "Put the selected objects in a new Folder."),
    def(A::Ungroup, "tree.ungroup", "Ungroup", SCENE_TREE, Tree, Press, &["Alt+G"], "Dissolve the selected Models / Folders, keeping their contents."),
    def(A::FindSelected, "tree.find", "Find selected", SCENE_TREE, Tree, Press, &["F"], "Scroll the tree to the selected object."),
    def(A::Rename, "tree.rename", "Rename", SCENE_TREE, Tree, Press, &["Ctrl+R"], "Rename the selected object in place."),
    // Windows (the two settings windows are not docked; they have their own shortcuts above).
    window(Panel::Viewport),
    window(Panel::Explorer),
    window(Panel::Properties),
    window(Panel::Output),
    window(Panel::Assets),
    window(Panel::ScriptEditor),
    def(A::RestoreLayout, "window.restore_layout", "Restore default layout", WINDOWS, Any, Press, &["Alt+0"], "Put every window back where the editor starts with it. Open settings windows stay open."),
    // Debug overlays: the same switches as the Debug menu above the viewport.
    debug(Overlay::Fps),
    debug(Overlay::FpsGraph),
    debug(Overlay::FrameTime),
    debug(Overlay::FrameTimeGraph),
    debug(Overlay::FrameBudget),
    debug(Overlay::OnePercentLow),
    debug(Overlay::FrameBreakdown),
    debug(Overlay::InstanceCount),
    debug(Overlay::DrawCalls),
    debug(Overlay::Triangles),
    debug(Overlay::Vertices),
    debug(Overlay::Lights),
    debug(Overlay::RenderStats),
    debug(Overlay::ScriptPerformance),
    debug(Overlay::EngineTimings),
    debug(Overlay::Physics),
    debug(Overlay::Memory),
    debug(Overlay::Network),
    debug(Overlay::AssetLoading),
    debug(Overlay::Warnings),
];

impl Action {
    /// The shortcuts that fly the camera (the secondary speed key alone does not move it).
    pub fn is_movement(self) -> bool {
        matches!(self, Action::CamForward | Action::CamBack | Action::CamLeft | Action::CamRight | Action::CamUp | Action::CamDown)
    }

    pub fn def(self) -> &'static ActionDef {
        DEFS.iter().find(|d| d.action == self).expect("every action has a definition")
    }

    pub fn from_id(id: &str) -> Option<Action> {
        DEFS.iter().find(|d| d.id == id).map(|d| d.action)
    }
}

// ---- chords ---------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Trigger {
    Key(Key),
    Wheel,
    /// Only modifiers (a held Shift).
    Modifiers,
}

/// A key (or the wheel) together with the modifiers that must be held.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    pub trigger: Trigger,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub fn key(key: Key, m: Modifiers) -> Chord {
        Chord { trigger: Trigger::Key(key), ctrl: m.command || m.ctrl, shift: m.shift, alt: m.alt }
    }

    pub fn wheel(m: Modifiers) -> Chord {
        Chord { trigger: Trigger::Wheel, ctrl: m.command || m.ctrl, shift: m.shift, alt: m.alt }
    }

    pub fn modifiers_only(m: Modifiers) -> Option<Chord> {
        let c = Chord { trigger: Trigger::Modifiers, ctrl: m.command || m.ctrl, shift: m.shift, alt: m.alt };
        (c.ctrl || c.shift || c.alt).then_some(c)
    }

    /// Reads `Ctrl+Shift+G`, `Shift+Scroll`, `Shift`, `Backtick`, `F5`... (`None` for nonsense).
    pub fn parse(text: &str) -> Option<Chord> {
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        let mut trigger = None;
        for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                "scroll" | "wheel" => trigger = Some(Trigger::Wheel),
                _ => trigger = Some(Trigger::Key(Key::from_name(part)?)),
            }
        }
        let trigger = trigger.unwrap_or(Trigger::Modifiers);
        let chord = Chord { trigger, ctrl, shift, alt };
        (trigger != Trigger::Modifiers || ctrl || shift || alt).then_some(chord)
    }

    /// The text stored in the settings file (round-trips through [`Chord::parse`]).
    pub fn to_storage(self) -> String {
        self.render(|k| k.name().to_owned())
    }

    /// The text shown to people.
    pub fn display(self) -> String {
        self.render(|k| k.symbol_or_name().to_owned())
    }

    fn render(self, key_name: impl Fn(Key) -> String) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        match self.trigger {
            Trigger::Key(k) => parts.push(key_name(k)),
            Trigger::Wheel => parts.push("Scroll".into()),
            Trigger::Modifiers => {}
        }
        parts.join("+")
    }

    fn modifiers_exactly(self, m: Modifiers) -> bool {
        (m.command || m.ctrl) == self.ctrl && m.shift == self.shift && m.alt == self.alt
    }

    /// For held shortcuts: the chord's modifiers are all held, and no Ctrl the chord does not name (so
    /// holding S while pressing Ctrl+S does not also fly the camera backwards). Extra Shift is fine: it
    /// switches to the second speed. So is extra Alt: it is the modifier of the fly-speed wheel and of the
    /// orbit, and holding it must never stop the camera from moving.
    fn modifiers_held(self, m: Modifiers) -> bool {
        let ctrl = m.command || m.ctrl;
        (!self.ctrl || ctrl) && (!self.shift || m.shift) && (!self.alt || m.alt) && (self.ctrl || !ctrl)
    }
}

// ---- the keymap -----------------------------------------------------------------------------

/// The bindings in effect: the starter ones plus the user's changes. A shortcut can have several keys.
#[derive(Clone, Debug)]
pub struct Keymap {
    binds: Vec<Vec<Chord>>,
    /// Debug shortcuts also fire while a text field has the keyboard (off: they would interfere with
    /// typing). Set from the debug settings every frame; not part of the bindings.
    pub debug_while_typing: bool,
}

fn starters(a: Action) -> Vec<Chord> {
    a.def().defaults.iter().filter_map(|text| Chord::parse(text)).collect()
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap { binds: DEFS.iter().map(|d| starters(d.action)).collect(), debug_while_typing: false }
    }
}

fn index(a: Action) -> usize {
    DEFS.iter().position(|d| d.action == a).expect("every action has a definition")
}

/// Why a key could not be added to a shortcut.
#[derive(Debug, PartialEq, Eq)]
pub enum AddError {
    /// The shortcut already has this key.
    AlreadyHere,
    /// Another shortcut that could be active at the same time has it.
    UsedBy(Action),
}

impl Keymap {
    /// The keys of a shortcut (empty = unbound).
    pub fn get(&self, a: Action) -> &[Chord] {
        &self.binds[index(a)]
    }

    pub fn has(&self, a: Action, chord: Chord) -> bool {
        self.get(a).contains(&chord)
    }

    /// Replaces all the keys of a shortcut.
    pub fn set(&mut self, a: Action, chords: Vec<Chord>) {
        let mut unique: Vec<Chord> = Vec::new();
        for c in chords {
            if !unique.contains(&c) {
                unique.push(c);
            }
        }
        self.binds[index(a)] = unique;
    }

    /// Gives a shortcut one more key, unless it would clash.
    pub fn add(&mut self, a: Action, chord: Chord) -> Result<(), AddError> {
        if self.has(a, chord) {
            return Err(AddError::AlreadyHere);
        }
        if let Some(other) = self.conflict(a, chord) {
            return Err(AddError::UsedBy(other));
        }
        self.binds[index(a)].push(chord);
        Ok(())
    }

    /// Replaces one key of a shortcut by another (the others stay), unless the new one would clash.
    pub fn replace(&mut self, a: Action, old: Chord, new: Chord) -> Result<(), AddError> {
        if old == new {
            return Ok(());
        }
        if self.has(a, new) {
            return Err(AddError::AlreadyHere);
        }
        if let Some(other) = self.conflict(a, new) {
            return Err(AddError::UsedBy(other));
        }
        match self.binds[index(a)].iter_mut().find(|c| **c == old) {
            Some(slot) => *slot = new,
            None => self.binds[index(a)].push(new),
        }
        Ok(())
    }

    /// Takes one key off a shortcut; the others stay.
    pub fn remove(&mut self, a: Action, chord: Chord) {
        self.binds[index(a)].retain(|c| *c != chord);
    }

    pub fn is_default(&self, a: Action) -> bool {
        self.get(a) == starters(a).as_slice()
    }

    pub fn reset(&mut self, a: Action) {
        self.binds[index(a)] = starters(a);
    }

    pub fn reset_all(&mut self) {
        *self = Keymap { debug_while_typing: self.debug_while_typing, ..Keymap::default() };
    }

    /// The starter keys of a shortcut.
    pub fn default_of(a: Action) -> Vec<Chord> {
        starters(a)
    }

    /// Another shortcut that would fire together with `a` if `a` had `chord`.
    pub fn conflict(&self, a: Action, chord: Chord) -> Option<Action> {
        let mine = a.def();
        DEFS.iter().filter(|d| d.action != a && self.has(d.action, chord) && d.context.overlaps(mine.context)).map(|d| d.action).next()
    }

    /// Whether one of this shortcut's keys is also used by a shortcut it could fire together with.
    pub fn clashes(&self, a: Action, chord: Chord) -> bool {
        self.conflict(a, chord).is_some()
    }

    /// Only what differs from the starter keys: `id -> keys` (an empty list = unbound).
    pub fn overrides(&self) -> BTreeMap<String, Vec<String>> {
        DEFS.iter().filter(|d| !self.is_default(d.action)).map(|d| (d.id.to_owned(), self.get(d.action).iter().map(|c| c.to_storage()).collect())).collect()
    }

    /// The starter keys with a saved set of changes applied. Entries naming shortcuts this version
    /// does not have, or keys that do not parse, are ignored.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_overrides(saved: &BTreeMap<String, Vec<String>>) -> Keymap {
        Self::with_overrides_noted(saved).0
    }

    /// Like [`Keymap::with_overrides`], and says what it had to change. A shortcut added in a newer version starts
    /// with keys the user may already have given to something else in their saved settings: the user's choice always
    /// wins, so the new shortcut starts without that key (it can be given another one, or the other shortcut changed,
    /// in the Shortcuts tab), and each case is explained in the notes.
    pub fn with_overrides_noted(saved: &BTreeMap<String, Vec<String>>) -> (Keymap, Vec<String>) {
        let mut map = Keymap::default();
        let mut chosen: HashSet<Action> = HashSet::new();
        for (id, keys) in saved {
            let Some(action) = Action::from_id(id) else { continue };
            let chords: Vec<Chord> = keys.iter().filter_map(|k| Chord::parse(k)).collect();
            if chords.is_empty() && !keys.is_empty() {
                continue; // nothing in it was readable: keep the starter keys
            }
            map.set(action, chords);
            chosen.insert(action);
        }
        let mut notes = Vec::new();
        for d in DEFS.iter().filter(|d| !chosen.contains(&d.action)) {
            for chord in map.get(d.action).to_vec() {
                let taken = DEFS.iter().find(|o| o.action != d.action && chosen.contains(&o.action) && map.has(o.action, chord) && o.context.overlaps(d.context));
                if let Some(other) = taken {
                    map.remove(d.action, chord);
                    notes.push(format!("{} is the starting key of '{}', but you use it for '{}': '{}' starts without it (Editor Settings > Shortcuts).", chord.display(), d.name, other.name, d.name));
                }
            }
        }
        (map, notes)
    }

    /// Reads the keyboard and mouse wheel for this frame.
    ///
    /// `hover` is the window the mouse is over and `typing` whether a text field has the keyboard.
    pub fn poll(&self, input: &egui::InputState, hover: Context, typing: bool) -> Frame {
        let mods = input.modifiers;
        let mut frame = Frame::default();
        let mut pressed: Vec<(Action, Chord)> = Vec::new();
        // Held keys that only count because Alt is tolerated (the chord does not name Alt).
        let mut alt_extra: Vec<(Action, Key)> = Vec::new();

        for d in DEFS {
            let works_while_typing = d.while_typing || (self.debug_while_typing && matches!(d.action, Action::Debug(_)));
            if !d.context.active_over(hover) || (typing && !works_while_typing) {
                continue;
            }
            for chord in self.get(d.action) {
                let chord = *chord;
                match (d.kind, chord.trigger) {
                    (Kind::Press, Trigger::Key(k)) => {
                        if input.key_pressed(k) && chord.modifiers_exactly(mods) {
                            pressed.push((d.action, chord));
                        }
                    }
                    (Kind::Hold, Trigger::Key(k)) => {
                        if input.key_down(k) && chord.modifiers_held(mods) {
                            frame.held.insert(d.action);
                            if mods.alt && !chord.alt {
                                alt_extra.push((d.action, k));
                            }
                        }
                    }
                    (Kind::Hold, Trigger::Modifiers) => {
                        if chord.modifiers_held(mods) {
                            frame.held.insert(d.action);
                        }
                    }
                    (Kind::Wheel, Trigger::Wheel) => {
                        if scrolled(input) && chord.modifiers_exactly(mods) {
                            frame.wheel = Some(d.action);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Alt + a movement key just pressed is a shortcut (Alt+S opens the settings), not a fly command.
        for (action, key) in alt_extra {
            if pressed.iter().any(|(_, c)| c.alt && c.trigger == Trigger::Key(key)) {
                frame.held.remove(&action);
            }
        }

        // The same keys bound in a specific window and in "anywhere": the specific one wins.
        for (action, chord) in &pressed {
            let shadowed = action.def().context == Context::Global && pressed.iter().any(|(other, c)| c == chord && other.def().context != Context::Global);
            if !shadowed && !frame.pressed.contains(action) {
                frame.pressed.push(*action);
            }
        }
        frame
    }
}

/// What the keyboard and wheel asked for this frame.
#[derive(Default, Clone, Debug)]
pub struct Frame {
    pub pressed: Vec<Action>,
    pub held: HashSet<Action>,
    /// The wheel shortcut that applies to the scrolling that happened this frame.
    pub wheel: Option<Action>,
}

impl Frame {
    pub fn held(&self, a: Action) -> bool {
        self.held.contains(&a)
    }

    #[cfg(test)]
    pub fn pressed(&self, a: Action) -> bool {
        self.pressed.contains(&a)
    }
}

/// The wheel turned this frame. (With Shift held egui reports the turn sideways, so either axis counts.)
fn scrolled(input: &egui::InputState) -> bool {
    input.smooth_scroll_delta != egui::Vec2::ZERO || input.events.iter().any(|e| matches!(e, egui::Event::MouseWheel { .. }))
}

// ---- capturing a new binding --------------------------------------------------------------------

/// The result of watching the keyboard while the user records a new binding.
#[derive(Debug, PartialEq)]
pub enum Captured {
    /// Nothing decisive yet.
    Waiting,
    Cancelled,
    Chord(Chord),
}

/// Looks at this frame's input for the next binding of an action of `kind`. `held` carries the
/// modifiers seen so far (a lone modifier is only a binding once it is released again).
pub fn capture(input: &egui::InputState, kind: Kind, held: &mut Modifiers) -> Captured {
    for event in &input.events {
        if let egui::Event::Key { key, pressed: true, modifiers, .. } = event {
            if *key == Key::Escape {
                return Captured::Cancelled;
            }
            if kind != Kind::Wheel {
                return Captured::Chord(Chord::key(*key, *modifiers));
            }
        }
    }
    if kind == Kind::Wheel && scrolled(input) {
        return Captured::Chord(Chord::wheel(input.modifiers));
    }
    if kind == Kind::Hold {
        let m = input.modifiers;
        if m.shift || m.ctrl || m.command || m.alt {
            held.shift |= m.shift;
            held.ctrl |= m.ctrl || m.command;
            held.alt |= m.alt;
        } else if let Some(chord) = Chord::modifiers_only(*held) {
            return Captured::Chord(chord);
        }
    }
    Captured::Waiting
}

#[cfg(test)]
mod tests {
    use forge_engine::compat::RunFrame as _;
    use super::*;

    /// Runs one egui frame with the given events and hands the input state to `f`.
    fn with_input<R>(modifiers: Modifiers, events: Vec<egui::Event>, scroll: f32, f: impl FnOnce(&egui::InputState) -> R) -> R {
        let ctx = egui::Context::default();
        let mut out = None;
        let mut f = Some(f);
        let mut events = events;
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut raw = egui::RawInput { events, ..Default::default() };
        if scroll != 0.0 {
            raw.events.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, scroll), phase: egui::TouchPhase::Move, modifiers });
        }
        let _ = ctx.run_frame(raw, |ctx| {
            ctx.input(|i| out = Some((f.take().unwrap())(i)));
        });
        out.unwrap()
    }

    fn key(k: Key, m: Modifiers) -> egui::Event {
        egui::Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m }
    }

    fn ctrl() -> Modifiers {
        Modifiers { ctrl: true, command: true, ..Default::default() }
    }

    fn c(text: &str) -> Chord {
        Chord::parse(text).unwrap_or_else(|| panic!("'{text}' is not a chord"))
    }

    fn texts(map: &Keymap, a: Action) -> Vec<String> {
        map.get(a).iter().map(|c| c.to_storage()).collect()
    }

    #[test]
    fn every_action_has_exactly_one_definition_and_valid_starter_keys() {
        let mut ids = HashSet::new();
        for d in DEFS {
            assert!(ids.insert(d.id), "duplicate id {}", d.id);
            assert_eq!(d.action.def().id, d.id);
            assert!(!d.description.is_empty() && !d.name.is_empty(), "{} needs a name and a description", d.id);
            for text in d.defaults {
                let chord = Chord::parse(text).unwrap_or_else(|| panic!("{}: starter key '{}' does not parse", d.id, text));
                let ok = match d.kind {
                    Kind::Wheel => chord.trigger == Trigger::Wheel,
                    Kind::Press => matches!(chord.trigger, Trigger::Key(_)),
                    Kind::Hold => matches!(chord.trigger, Trigger::Key(_) | Trigger::Modifiers),
                };
                assert!(ok, "{}: '{}' is not a valid key for its kind", d.id, text);
            }
        }
    }

    #[test]
    fn starter_keys_do_not_conflict() {
        let map = Keymap::default();
        for d in DEFS {
            for chord in map.get(d.action) {
                assert_eq!(map.conflict(d.action, *chord), None, "{} conflicts with another starter key", d.id);
            }
        }
    }

    #[test]
    fn the_starter_list_matches_the_design_notes() {
        let map = Keymap::default();
        assert_eq!(texts(&map, Action::CamForward), ["W"]);
        assert_eq!(texts(&map, Action::SnapTo), ["F"]);
        assert_eq!(texts(&map, Action::FastSpeedWheel), ["Shift+Scroll"]);
        assert_eq!(texts(&map, Action::PrimarySpeedWheel), ["Alt+Scroll"]);
        assert_eq!(texts(&map, Action::ZoomWheel), ["Scroll"]);
        assert_eq!(texts(&map, Action::InsertObject), ["Shift+Space"]);
        assert_eq!(texts(&map, Action::ToolRotate), ["4"]);
        assert_eq!(texts(&map, Action::MoveIncrement), ["Shift+2"]);
        assert_eq!(texts(&map, Action::Folder), ["Ctrl+Shift+G"]);
        assert_eq!(texts(&map, Action::Ungroup), ["Alt+G"]);
        assert_eq!(texts(&map, Action::Redo), ["Ctrl+Shift+Z", "Ctrl+Y"], "a shortcut may start with several keys");
        assert_eq!(texts(&map, Action::EditorSettings), ["Alt+S"]);
        assert_eq!(texts(&map, Action::ProjectSettings), ["Ctrl+Alt+S"]);
        // Ctrl+R means two things, depending on the window.
        assert_eq!(map.get(Action::Rotate90Y), map.get(Action::Rename));
        assert_eq!(Action::Rotate90Y.def().context, Context::Viewport);
        assert_eq!(Action::Rename.def().context, Context::Explorer);
    }

    #[test]
    fn chords_round_trip_through_text_for_every_key() {
        for key in Key::ALL {
            let chord = Chord::key(*key, Modifiers { ctrl: true, command: true, shift: true, alt: true, ..Default::default() });
            let text = chord.to_storage();
            assert_eq!(Chord::parse(&text), Some(chord), "{key:?} -> '{text}'");
            assert!(!text.trim_start_matches("Ctrl+Shift+Alt+").contains('+'), "{key:?} has a '+' in its name");
        }
        assert_eq!(Chord::parse("Shift"), Chord::modifiers_only(Modifiers { shift: true, ..Default::default() }));
        assert_eq!(Chord::parse("ctrl+shift+scroll").map(Chord::to_storage).as_deref(), Some("Ctrl+Shift+Scroll"));
        assert_eq!(Chord::parse("nonsense"), None);
        assert_eq!(Chord::parse(""), None);
    }

    #[test]
    fn a_shortcut_can_have_several_keys_and_one_can_be_removed_without_touching_the_rest() {
        let mut map = Keymap::default();
        assert_eq!(map.add(Action::Save, c("Ctrl+Alt+W")), Ok(()));
        assert_eq!(texts(&map, Action::Save), ["Ctrl+S", "Ctrl+Alt+W"]);
        assert_eq!(map.add(Action::Save, c("Ctrl+Alt+W")), Err(AddError::AlreadyHere), "no duplicates on one shortcut");
        assert_eq!(map.add(Action::Save, c("Ctrl+C")), Err(AddError::UsedBy(Action::Copy)), "another shortcut has it");

        map.remove(Action::Save, c("Ctrl+S"));
        assert_eq!(texts(&map, Action::Save), ["Ctrl+Alt+W"], "only the key under the cursor went");
        assert_eq!(texts(&map, Action::Redo), ["Ctrl+Shift+Z", "Ctrl+Y"], "other shortcuts are untouched");
        map.remove(Action::Save, c("Ctrl+Alt+W"));
        assert!(map.get(Action::Save).is_empty(), "a shortcut can end up unbound");
    }

    #[test]
    fn replacing_a_key_keeps_the_others() {
        let mut map = Keymap::default();
        assert_eq!(map.replace(Action::Redo, c("Ctrl+Y"), c("Ctrl+Alt+Y")), Ok(()));
        assert_eq!(texts(&map, Action::Redo), ["Ctrl+Shift+Z", "Ctrl+Alt+Y"]);
        assert_eq!(map.replace(Action::Redo, c("Ctrl+Alt+Y"), c("Ctrl+Shift+Z")), Err(AddError::AlreadyHere));
        assert_eq!(map.replace(Action::Redo, c("Ctrl+Alt+Y"), c("Ctrl+Z")), Err(AddError::UsedBy(Action::Undo)));
    }

    #[test]
    fn changes_are_saved_as_overrides_only_and_come_back() {
        let mut map = Keymap::default();
        assert!(map.overrides().is_empty());
        map.add(Action::Save, c("Ctrl+Alt+Shift+S")).unwrap();
        map.set(Action::Undo, vec![]);
        let saved = map.overrides();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved["general.undo"], Vec::<String>::new(), "unbound is saved as an empty list");
        let back = Keymap::with_overrides(&saved);
        assert_eq!(back.get(Action::Save), [c("Ctrl+S"), c("Ctrl+Alt+Shift+S")]);
        assert!(back.get(Action::Undo).is_empty());
        assert_eq!(back.get(Action::Copy), Keymap::default().get(Action::Copy));

        let mut junk = BTreeMap::new();
        junk.insert("no.such.action".to_owned(), vec!["X".to_owned()]);
        junk.insert("general.save".to_owned(), vec!["banana+banana".to_owned()]);
        assert!(Keymap::with_overrides(&junk).is_default(Action::Save), "unreadable entries fall back to the starter keys");
    }

    #[test]
    fn conflicts_only_count_between_windows_that_can_be_active_together() {
        let map = Keymap::default();
        assert_eq!(map.conflict(Action::Rename, c("Ctrl+R")), None, "viewport and explorer never overlap");
        assert_eq!(map.conflict(Action::Rename, c("Ctrl+S")), Some(Action::Save), "a window shortcut would shadow an anywhere shortcut");
    }

    #[test]
    fn the_window_under_the_mouse_decides() {
        let map = Keymap::default();
        let press = |hover| with_input(ctrl(), vec![key(Key::R, ctrl())], 0.0, |i| map.poll(i, hover, false));
        assert!(press(Context::Viewport).pressed(Action::Rotate90Y) && !press(Context::Viewport).pressed(Action::Rename));
        assert!(press(Context::Explorer).pressed(Action::Rename) && !press(Context::Explorer).pressed(Action::Rotate90Y));
        assert!(press(Context::Properties).pressed.is_empty(), "neither applies over the Properties panel");
    }

    #[test]
    fn any_of_a_shortcuts_keys_triggers_it() {
        let map = Keymap::default();
        let y = with_input(ctrl(), vec![key(Key::Y, ctrl())], 0.0, |i| map.poll(i, Context::Properties, false));
        assert!(y.pressed(Action::Redo), "the second starter key of Redo works");
        let z = Modifiers { ctrl: true, command: true, shift: true, ..Default::default() };
        let both = with_input(z, vec![key(Key::Z, z)], 0.0, |i| map.poll(i, Context::Properties, false));
        assert_eq!(both.pressed.iter().filter(|a| **a == Action::Redo).count(), 1, "one press fires it once");
    }

    #[test]
    fn repeat_duplicate_is_ctrl_shift_d_next_to_duplicate_and_the_two_never_fire_together() {
        let map = Keymap::default();
        assert_eq!(texts(&map, Action::RepeatDuplicate), ["Ctrl+Shift+D"]);
        assert_eq!(texts(&map, Action::Duplicate), ["Ctrl+D"], "Duplicate keeps its key");
        assert_eq!(Action::RepeatDuplicate.def().context, Context::Global);
        assert_eq!(map.conflict(Action::RepeatDuplicate, c("Ctrl+Shift+D")), None, "no other shortcut starts with it");
        let shift = Modifiers { ctrl: true, command: true, shift: true, ..Default::default() };
        let repeat = with_input(shift, vec![key(Key::D, shift)], 0.0, |i| map.poll(i, Context::Viewport, false));
        assert!(repeat.pressed(Action::RepeatDuplicate) && !repeat.pressed(Action::Duplicate) && !repeat.held(Action::CamRight), "Ctrl+Shift+D is only the repeat (and does not fly the camera sideways)");
        let plain = with_input(ctrl(), vec![key(Key::D, ctrl())], 0.0, |i| map.poll(i, Context::Viewport, false));
        assert!(plain.pressed(Action::Duplicate) && !plain.pressed(Action::RepeatDuplicate));
        let typing = with_input(shift, vec![key(Key::D, shift)], 0.0, |i| map.poll(i, Context::Viewport, true));
        assert!(typing.pressed.is_empty(), "not while a text field has the keyboard");
    }

    #[test]
    fn a_new_starter_key_never_takes_a_key_the_user_already_uses_and_says_so() {
        // A user who gave Ctrl+Shift+D to something else (here Save, which works anywhere) before it was a starter key.
        let mut saved = BTreeMap::new();
        saved.insert("general.save".to_owned(), vec!["Ctrl+Shift+D".to_owned()]);
        let (map, notes) = Keymap::with_overrides_noted(&saved);
        assert_eq!(texts(&map, Action::Save), ["Ctrl+Shift+D"], "what the user chose stays");
        assert!(map.get(Action::RepeatDuplicate).is_empty(), "the new shortcut starts without the key");
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("Repeat duplicate") && notes[0].contains("Save project") && notes[0].contains("Ctrl+Shift+D"), "{notes:?}");
        assert_eq!(map.conflict(Action::Save, c("Ctrl+Shift+D")), None, "and nothing clashes now");
        // It can be given another key in the Shortcuts tab; the key it was refused is refused there too, with the owner named.
        let mut map = map;
        assert_eq!(map.add(Action::RepeatDuplicate, c("Ctrl+Shift+D")), Err(AddError::UsedBy(Action::Save)));
        map.add(Action::RepeatDuplicate, c("Ctrl+Alt+D")).unwrap();
        // The same applies when the user's key is in a window of its own (it still overlaps "anywhere").
        let mut viewport_only = BTreeMap::new();
        viewport_only.insert("tool.rotate_y".to_owned(), vec!["Ctrl+Shift+D".to_owned()]);
        let (map, notes) = Keymap::with_overrides_noted(&viewport_only);
        assert!(map.get(Action::RepeatDuplicate).is_empty() && notes.len() == 1);
        // No clash, no change and no note; and a user who set their own key for it keeps that key.
        let (map, notes) = Keymap::with_overrides_noted(&BTreeMap::new());
        assert!(map.is_default(Action::RepeatDuplicate) && notes.is_empty());
        let mut own = BTreeMap::new();
        own.insert("general.repeat_duplicate".to_owned(), vec!["Ctrl+Alt+R".to_owned()]);
        let (map, notes) = Keymap::with_overrides_noted(&own);
        assert_eq!(texts(&map, Action::RepeatDuplicate), ["Ctrl+Alt+R"]);
        assert!(notes.is_empty());
        // The settings file keeps what a note changed (an unbound shortcut is saved as an empty list).
        let (map, _) = Keymap::with_overrides_noted(&saved);
        assert_eq!(map.overrides().get("general.repeat_duplicate"), Some(&Vec::<String>::new()));
    }

    #[test]
    fn general_shortcuts_work_over_any_window_and_typing_blocks_most() {
        let map = Keymap::default();
        let save = |hover, typing| with_input(ctrl(), vec![key(Key::S, ctrl())], 0.0, |i| map.poll(i, hover, typing));
        assert!(save(Context::Properties, false).pressed(Action::Save));
        assert!(save(Context::Explorer, true).pressed(Action::Save), "saving works from inside a text field");
        let copy = with_input(ctrl(), vec![key(Key::C, ctrl())], 0.0, |i| map.poll(i, Context::Viewport, true));
        assert!(copy.pressed.is_empty(), "Ctrl+C belongs to the text field being typed in");
    }

    #[test]
    fn window_shortcuts_work_over_any_window_but_never_while_typing_and_never_clash() {
        let map = Keymap::default();
        let alt = Modifiers { alt: true, ..Default::default() };
        let press = |k: Key, hover, typing| with_input(alt, vec![key(k, alt)], 0.0, |i| map.poll(i, hover, typing));
        for (k, panel) in [(Key::Num1, Panel::Viewport), (Key::Num2, Panel::Explorer), (Key::Num3, Panel::Properties), (Key::Num4, Panel::Output), (Key::Num5, Panel::Assets), (Key::Num6, Panel::ScriptEditor)] {
            for hover in [Context::Viewport, Context::Explorer, Context::Properties, Context::Global] {
                assert!(press(k, hover, false).pressed(Action::Window(panel)), "{panel:?} over {hover:?}");
            }
            assert!(press(k, Context::Viewport, true).pressed.is_empty(), "typing in a text field or the script editor keeps {panel:?}'s key for itself");
        }
        assert!(press(Key::Num0, Context::Properties, false).pressed(Action::RestoreLayout));
        // The tools' own digits in the viewport are plain digits: the window shortcuts (Alt + digit) do not shadow them.
        let one = with_input(Modifiers::default(), vec![key(Key::Num1, Modifiers::default())], 0.0, |i| map.poll(i, Context::Viewport, false));
        assert!(one.pressed(Action::ToolSelect) && !one.pressed(Action::Window(Panel::Viewport)));

        // A key already used where it would fire is refused when a window shortcut is rebound to it.
        let mut map = Keymap::default();
        assert_eq!(map.add(Action::Window(Panel::Explorer), c("Ctrl+S")), Err(AddError::UsedBy(Action::Save)));
        assert_eq!(map.add(Action::Window(Panel::Explorer), c("Alt+3")), Err(AddError::UsedBy(Action::Window(Panel::Properties))));
        assert_eq!(map.add(Action::Window(Panel::Explorer), c("Alt+F1")), Ok(()), "a free key is accepted");
    }

    #[test]
    fn window_shortcuts_can_be_changed_saved_and_reset_like_any_other() {
        let mut map = Keymap::default();
        map.set(Action::Window(Panel::Assets), vec![c("Ctrl+Alt+A")]);
        assert!(!map.is_default(Action::Window(Panel::Assets)));
        let saved = map.overrides();
        assert_eq!(saved.get("window.assets"), Some(&vec!["Ctrl+Alt+A".to_owned()]));
        let back = Keymap::with_overrides(&saved);
        assert_eq!(back.get(Action::Window(Panel::Assets)), [c("Ctrl+Alt+A")]);
        assert!(back.is_default(Action::Window(Panel::Explorer)));
        let mut again = back;
        again.reset(Action::Window(Panel::Assets));
        assert_eq!(again.get(Action::Window(Panel::Assets)), [c("Alt+5")]);
        // Every panel with a window shortcut appears in the settings table under its own category.
        assert!(DEFS.iter().filter(|d| d.category == "Windows").count() >= 7);
    }

    #[test]
    fn modifiers_must_match_exactly_for_presses() {
        let map = Keymap::default();
        let shift_ctrl = Modifiers { ctrl: true, command: true, shift: true, ..Default::default() };
        let f = with_input(shift_ctrl, vec![key(Key::Z, shift_ctrl)], 0.0, |i| map.poll(i, Context::Viewport, false));
        assert!(f.pressed(Action::Redo) && !f.pressed(Action::Undo));
    }

    #[test]
    fn camera_keys_work_while_hovering_the_viewport_and_with_shift_held() {
        let map = Keymap::default();
        let shift = Modifiers { shift: true, ..Default::default() };
        let ctx = egui::Context::default();
        let mut result = None;
        // W is held down (a key event keeps it in `keys_down`), Shift is held as well.
        let raw = egui::RawInput { events: vec![egui::Event::ModifiersChanged(shift), key(Key::W, shift)], ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| {
            ctx.input(|i| result = Some((map.poll(i, Context::Viewport, false), map.poll(i, Context::Explorer, false))));
        });
        let (viewport, explorer) = result.unwrap();
        assert!(viewport.held(Action::CamForward) && viewport.held(Action::FastCamera), "no mouse button is needed to move");
        assert!(!explorer.held(Action::CamForward), "the camera keys only work over the viewport");
    }

    #[test]
    fn holding_alt_never_stops_the_camera_from_flying_in_any_direction() {
        let map = Keymap::default();
        let alt = Modifiers { alt: true, ..Default::default() };
        for (k, action) in [(Key::W, Action::CamForward), (Key::A, Action::CamLeft), (Key::S, Action::CamBack), (Key::D, Action::CamRight), (Key::E, Action::CamUp), (Key::Q, Action::CamDown)] {
            for mods in [Modifiers::default(), alt, Modifiers { alt: true, shift: true, ..Default::default() }] {
                let held = with_input(mods, vec![key(k, mods)], 0.0, |i| map.poll(i, Context::Viewport, false));
                // (Alt+S is also the Editor Settings shortcut: pressed fresh it opens the window instead of flying back.)
                if mods == alt && k == Key::S {
                    assert!(held.pressed(Action::EditorSettings) && !held.held(action), "Alt+S is a shortcut, not a fly command");
                } else {
                    assert!(held.held(action), "{action:?} with {mods:?}");
                }
            }
        }
        // Ctrl still keeps the fly keys out of the way of Ctrl shortcuts.
        let held = with_input(ctrl(), vec![key(Key::S, ctrl())], 0.0, |i| map.poll(i, Context::Viewport, false));
        assert!(!held.held(Action::CamBack) && held.pressed(Action::Save));
    }

    #[test]
    fn the_wheel_actions_have_the_new_names_and_keep_their_ids_and_keys() {
        assert_eq!(Action::PrimarySpeedWheel.def().name, "Adjust Fly Camera Speed");
        assert_eq!(Action::FastSpeedWheel.def().name, "Adjust Secondary Camera Speed");
        assert_eq!(Action::PrimarySpeedWheel.def().id, "viewport.fly_speed", "saved bindings still find them");
        assert_eq!(Action::FastSpeedWheel.def().id, "viewport.fast_speed");
        assert_eq!(Action::from_id("viewport.fly_speed"), Some(Action::PrimarySpeedWheel));
        let saved = BTreeMap::from([("viewport.fly_speed".to_owned(), vec!["Ctrl+Scroll".to_owned()])]);
        let map = Keymap::with_overrides(&saved);
        assert_eq!(texts(&map, Action::PrimarySpeedWheel), ["Ctrl+Scroll"], "a binding saved under the old name still applies");
        assert!(DEFS.iter().all(|d| !d.name.contains("Change fly speed") && !d.name.contains("Change second camera speed")));
        for a in [Action::PrimarySpeedWheel, Action::FastSpeedWheel, Action::FastCamera] {
            assert!(!a.def().description.is_empty());
        }
    }
    #[test]
    fn the_wheel_picks_the_shortcut_with_matching_modifiers() {
        let map = Keymap::default();
        let plain = with_input(Modifiers::default(), vec![], 1.0, |i| map.poll(i, Context::Viewport, false));
        assert_eq!(plain.wheel, Some(Action::ZoomWheel));
        let shift = Modifiers { shift: true, ..Default::default() };
        let fast = with_input(shift, vec![], 1.0, |i| map.poll(i, Context::Viewport, false));
        assert_eq!(fast.wheel, Some(Action::FastSpeedWheel));
        let alt = Modifiers { alt: true, ..Default::default() };
        let primary = with_input(alt, vec![], 1.0, |i| map.poll(i, Context::Viewport, false));
        assert_eq!(primary.wheel, Some(Action::PrimarySpeedWheel));
        let elsewhere = with_input(Modifiers::default(), vec![], 1.0, |i| map.poll(i, Context::Explorer, false));
        assert_eq!(elsewhere.wheel, None);
    }

    #[test]
    fn capturing_reads_a_chord_cancels_on_escape_and_binds_a_lone_modifier_on_release() {
        let mut held = Modifiers::default();
        let got = with_input(ctrl(), vec![key(Key::G, ctrl())], 0.0, |i| capture(i, Kind::Press, &mut held));
        assert_eq!(got, Captured::Chord(c("Ctrl+G")));
        let esc = with_input(Modifiers::default(), vec![key(Key::Escape, Modifiers::default())], 0.0, |i| capture(i, Kind::Press, &mut held));
        assert_eq!(esc, Captured::Cancelled);

        let shift = Modifiers { shift: true, ..Default::default() };
        let mut held = Modifiers::default();
        assert_eq!(with_input(shift, vec![], 0.0, |i| capture(i, Kind::Hold, &mut held)), Captured::Waiting);
        let released = with_input(Modifiers::default(), vec![], 0.0, |i| capture(i, Kind::Hold, &mut held));
        assert_eq!(released, Captured::Chord(c("Shift")));
        let mut none = Modifiers::default();
        assert_eq!(with_input(shift, vec![], 0.0, |i| capture(i, Kind::Press, &mut none)), Captured::Waiting, "a press shortcut cannot be a lone modifier");
    }

    // ---- debug overlays ----

    fn ctrl_shift() -> Modifiers {
        Modifiers { ctrl: true, command: true, shift: true, ..Default::default() }
    }

    #[test]
    fn every_overlay_has_its_own_shortcut_and_only_show_fps_starts_bound() {
        let map = Keymap::default();
        for o in OVERLAYS {
            let action = Action::Debug(o.overlay);
            let def = action.def();
            assert_eq!((def.id, def.name, def.category), (o.key, o.name, "Debug"));
            assert_eq!(def.context, Context::Global, "{} works over any window", o.name);
            assert_eq!(Action::from_id(o.key), Some(action));
            if o.overlay == Overlay::Fps {
                assert_eq!(map.get(action), [c("Ctrl+Shift+F5")]);
            } else {
                assert!(map.get(action).is_empty(), "{} starts unbound", o.name);
            }
        }
        assert_eq!(DEFS.iter().filter(|d| d.category == "Debug").count(), OVERLAYS.len(), "nothing missing, nothing extra");
        let mut ids: Vec<_> = DEFS.iter().map(|d| d.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), DEFS.len(), "shortcut ids are unique");
    }

    #[test]
    fn ctrl_shift_f5_toggles_the_fps_and_leaves_play_and_stop_alone() {
        let map = Keymap::default();
        let f5 = |mods: Modifiers| with_input(mods, vec![key(Key::F5, mods)], 0.0, |i| map.poll(i, Context::Properties, false));
        let chord = f5(ctrl_shift());
        assert_eq!(chord.pressed, [Action::Debug(Overlay::Fps)]);
        assert_eq!(f5(Modifiers::default()).pressed, [Action::Play]);
        assert_eq!(f5(Modifiers { shift: true, ..Default::default() }).pressed, [Action::Stop]);
    }

    #[test]
    fn debug_shortcuts_keep_out_of_text_fields_unless_allowed() {
        let mut map = Keymap::default();
        map.add(Action::Debug(Overlay::Physics), c("Ctrl+Alt+P")).unwrap();
        let ctrl_alt = Modifiers { ctrl: true, command: true, alt: true, ..Default::default() };
        let press = |map: &Keymap, typing| {
            (
                with_input(ctrl_shift(), vec![key(Key::F5, ctrl_shift())], 0.0, |i| map.poll(i, Context::Viewport, typing)),
                with_input(ctrl_alt, vec![key(Key::P, ctrl_alt)], 0.0, |i| map.poll(i, Context::Viewport, typing)),
            )
        };
        let (fps, physics) = press(&map, false);
        assert!(fps.pressed(Action::Debug(Overlay::Fps)) && physics.pressed(Action::Debug(Overlay::Physics)));
        let (fps, physics) = press(&map, true);
        assert!(fps.pressed.is_empty() && physics.pressed.is_empty(), "typing a name must not toggle an overlay");
        map.debug_while_typing = true;
        let (fps, physics) = press(&map, true);
        assert!(fps.pressed(Action::Debug(Overlay::Fps)) && physics.pressed(Action::Debug(Overlay::Physics)), "unless the user asked for it");
        map.reset_all();
        assert!(map.debug_while_typing, "resetting the keys does not change that setting");
    }

    #[test]
    fn debug_shortcuts_can_be_assigned_changed_cleared_and_conflicts_are_found() {
        let mut map = Keymap::default();
        let memory = Action::Debug(Overlay::Memory);
        assert!(map.get(memory).is_empty(), "unbound by default");
        assert_eq!(map.add(memory, c("Ctrl+Shift+M")), Ok(()));
        assert_eq!(map.add(memory, c("Ctrl+Shift+M")), Err(AddError::AlreadyHere));
        assert_eq!(map.add(memory, c("Ctrl+Shift+F5")), Err(AddError::UsedBy(Action::Debug(Overlay::Fps))), "two overlays cannot share keys");
        assert_eq!(map.add(memory, c("Ctrl+S")), Err(AddError::UsedBy(Action::Save)), "nor can an overlay take a general shortcut");
        assert_eq!(map.replace(memory, c("Ctrl+Shift+M"), c("Ctrl+Alt+M")), Ok(()));
        assert_eq!(texts(&map, memory), ["Ctrl+Alt+M"]);
        // An unbound shortcut can be given the starting keys of another that was cleared.
        map.set(Action::Debug(Overlay::Fps), vec![]);
        assert_eq!(map.add(memory, c("Ctrl+Shift+F5")), Ok(()));
        // They are saved as overrides and come back.
        let saved = map.overrides();
        assert_eq!(saved["debug.memory"], ["Ctrl+Alt+M", "Ctrl+Shift+F5"]);
        assert_eq!(saved["debug.fps"], Vec::<String>::new(), "a cleared starter key is remembered as cleared");
        let back = Keymap::with_overrides(&saved);
        assert!(back.get(Action::Debug(Overlay::Fps)).is_empty());
        assert_eq!(texts(&back, memory), ["Ctrl+Alt+M", "Ctrl+Shift+F5"]);
        map.reset(Action::Debug(Overlay::Fps));
        assert_eq!(map.conflict(memory, c("Ctrl+Shift+F5")), Some(Action::Debug(Overlay::Fps)));
    }
}
