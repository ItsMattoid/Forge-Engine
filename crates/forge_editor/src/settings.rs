//! Editor settings: what belongs to the person using the editor, not to a project. Kept in the
//! user's config folder (`%APPDATA%\Forge\editor_settings.json` on Windows) and shared by every project.

use std::{collections::BTreeMap, path::PathBuf};

use forge_engine::{diag::DebugSettings, project::write_atomic};
use serde::{Deserialize, Serialize};

use crate::{keymap::Keymap, render_settings::RenderSettings, theme::Palette};

/// Whether the move and rotate tools work along the world's axes or the selection's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Space {
    #[default]
    World,
    Local,
}

impl Space {
    pub fn toggled(self) -> Space {
        match self {
            Space::World => Space::Local,
            Space::Local => Space::World,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Space::World => "World",
            Space::Local => "Local",
        }
    }
}

/// What the mouse wheel zooms toward.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ZoomMode {
    /// What is under the mouse pointer in the viewport (the first thing its ray hits); with nothing there,
    /// along that ray anyway, a sensible distance out. Never the selection.
    Hover,
    /// Along the direction the camera is looking, from where it is. Nothing else matters: not the selection,
    /// not the mouse position. (An unknown name in a file also reads as this.)
    #[default]
    #[serde(other)]
    Camera,
}

impl ZoomMode {
    pub const ALL: [ZoomMode; 2] = [ZoomMode::Camera, ZoomMode::Hover];

    pub fn label(self) -> &'static str {
        match self {
            ZoomMode::Camera => "Camera-Based Zoom",
            ZoomMode::Hover => "Hover-Based Zoom",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ZoomMode::Camera => "The wheel moves the camera forward and back along the direction it is looking, whatever is selected and wherever the mouse is.",
            ZoomMode::Hover => "The wheel moves the camera toward the point under the mouse pointer (what the pointer is over in the viewport; empty space zooms along the pointer's line of sight).",
        }
    }
}

/// How the Shortcuts tab (and the list of settings pages) is ordered.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum SettingsOrder {
    /// By the names shown, A to Z, to find one by name.
    Alphabetical,
    /// By how people use them: the everyday settings and shortcuts first, related ones together. (An unknown
    /// name in a file also reads as this.)
    #[default]
    #[serde(other)]
    Common,
}

impl SettingsOrder {
    pub const ALL: [SettingsOrder; 2] = [SettingsOrder::Common, SettingsOrder::Alphabetical];

    pub fn label(self) -> &'static str {
        match self {
            SettingsOrder::Common => "Common",
            SettingsOrder::Alphabetical => "Alphabetical",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SettingsOrder::Common => "Grouped by what they are for, the most used first.",
            SettingsOrder::Alphabetical => "Sorted by name, A to Z, within each group.",
        }
    }
}

/// Where a window that is not part of the dock was last, and how big (egui points, size of what is inside).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct WindowPlace {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A shortcut's saved keys: a list, or (older files) a single text where "" meant unbound.
#[derive(Deserialize)]
#[serde(untagged)]
enum SavedKeys {
    Many(Vec<String>),
    One(String),
}

fn keys_by_shortcut<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Vec<String>>, D::Error> {
    let raw = BTreeMap::<String, SavedKeys>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .map(|(id, keys)| {
            let list = match keys {
                SavedKeys::Many(list) => list,
                SavedKeys::One(one) if one.trim().is_empty() => Vec::new(),
                SavedKeys::One(one) => vec![one],
            };
            (id, list)
        })
        .collect())
}

/// The colours: a bad colour or a missing one keeps its default instead of spoiling the whole file.
fn palette_from_file<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Palette, D::Error> {
    Ok(serde_json::Value::deserialize(d)?.as_object().map_or_else(Palette::default, Palette::from_file))
}

/// The file's contents. Every field has a default, so a file from an older or newer Forge still loads.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    /// Shortcuts that differ from the starter keys (`id -> keys`, an empty list = unbound). Files from
    /// before shortcuts could have several keys hold one text instead of a list; both are read.
    #[serde(deserialize_with = "keys_by_shortcut")]
    shortcuts: BTreeMap<String, Vec<String>>,
    camera_speed: f32,
    fast_multiplier: f32,
    move_increment: f32,
    rotate_increment: f32,
    space: Space,
    click_cycle_tolerance: f32,
    /// Which debug overlays are on and how they look (the Debug menu above the viewport).
    debug: DebugSettings,
    /// V-Sync, the frame-rate limit, anti-aliasing.
    render: RenderSettings,
    /// The colours of the editor's windows and text.
    #[serde(deserialize_with = "palette_from_file")]
    theme: Palette,
    /// Where the editor's windows are (see `dock`): kept as it was written, read when the editor starts.
    layout: Option<serde_json::Value>,
    /// What the mouse wheel zooms toward.
    zoom_mode: ZoomMode,
    /// Where the Editor Settings and Project Settings windows were (`window name -> place`).
    window_places: BTreeMap<String, WindowPlace>,
    /// Common or alphabetical order of the shortcuts and settings pages.
    settings_order: SettingsOrder,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            shortcuts: BTreeMap::new(),
            camera_speed: 40.0,
            fast_multiplier: 3.0,
            move_increment: 1.0,
            rotate_increment: 15.0,
            space: Space::World,
            click_cycle_tolerance: 4.0,
            debug: DebugSettings::default(),
            render: RenderSettings::default(),
            theme: Palette::default(),
            layout: None,
            zoom_mode: ZoomMode::Camera,
            window_places: BTreeMap::new(),
            settings_order: SettingsOrder::Common,
        }
    }
}

/// Limits of the fly speed (studs per second) and of the second-speed multiplier. The multiplier may be
/// below 1 (a slow, precise mode); the floors keep the camera from becoming unusably slow or fast.
pub const MIN_FLY_SPEED: f32 = 0.1;
pub const MAX_FLY_SPEED: f32 = 1000.0;
pub const MIN_SECOND_SPEED: f32 = 0.05;
pub const MAX_SECOND_SPEED: f32 = 50.0;

pub struct EditorSettings {
    pub keymap: Keymap,
    /// Fly speed of the viewport camera, studs per second.
    pub camera_speed: f32,
    /// What the "second speed" key multiplies the fly speed by.
    pub fast_multiplier: f32,
    /// Grid the move tool snaps to (0 = free).
    pub move_increment: f32,
    /// Step of the rotate tool, in degrees (0 = free).
    pub rotate_increment: f32,
    pub space: Space,
    /// How far (pixels) the mouse may drift between two Alt-clicks and still count as the same spot.
    pub click_cycle_tolerance: f32,
    /// The debug overlays (see `forge_engine::diag`).
    pub debug: DebugSettings,
    pub render: RenderSettings,
    /// The colours of the editor (see `theme`).
    pub theme: Palette,
    /// The saved window layout (`None` until the layout is first changed).
    pub layout: Option<serde_json::Value>,
    /// What the mouse wheel zooms toward (see [`ZoomMode`]).
    pub zoom_mode: ZoomMode,
    /// The last place of the windows that are not docked (the settings windows), by name.
    pub window_places: BTreeMap<String, WindowPlace>,
    /// Common or alphabetical order of the shortcuts and settings pages (changes the order shown, nothing else).
    pub settings_order: SettingsOrder,
    /// Things worth telling the user about the settings that were just loaded (see Keymap::with_overrides_noted).
    pub startup_notes: Vec<String>,
    path: Option<PathBuf>,
    changed: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self::from_saved(Saved::default(), None)
    }
}

impl EditorSettings {
    /// Where the settings live. (Tests never touch the real file.)
    fn default_path() -> Option<PathBuf> {
        if cfg!(test) {
            return None;
        }
        dirs::config_dir().map(|d| d.join("Forge").join("editor_settings.json"))
    }

    /// Loads the settings; a missing or unreadable file gives the defaults (and a message to show).
    pub fn load() -> (Self, Option<String>) {
        Self::load_from(Self::default_path())
    }

    /// Loads the settings kept in a given file (for tests of what survives from one session to the next).
    #[cfg(test)]
    pub(crate) fn load_at(path: PathBuf) -> (Self, Option<String>) {
        Self::load_from(Some(path))
    }

    fn load_from(path: Option<PathBuf>) -> (Self, Option<String>) {
        let mut problem = None;
        let saved = match path.as_ref().map(std::fs::read_to_string) {
            Some(Ok(text)) => serde_json::from_str(&text).unwrap_or_else(|e| {
                problem = Some(format!("the editor settings file could not be read ({e}); using the defaults"));
                Saved::default()
            }),
            _ => Saved::default(),
        };
        (Self::from_saved(saved, path), problem)
    }

    fn from_saved(s: Saved, path: Option<PathBuf>) -> Self {
        let mut debug = s.debug;
        debug.sanitize();
        let (mut keymap, startup_notes) = Keymap::with_overrides_noted(&s.shortcuts);
        keymap.debug_while_typing = debug.shortcuts_while_typing;
        let mut render = s.render;
        render.sanitize();
        EditorSettings {
            keymap,
            camera_speed: s.camera_speed.clamp(MIN_FLY_SPEED, MAX_FLY_SPEED),
            fast_multiplier: s.fast_multiplier.clamp(MIN_SECOND_SPEED, MAX_SECOND_SPEED),
            move_increment: s.move_increment.clamp(0.0, 1000.0),
            rotate_increment: s.rotate_increment.clamp(0.0, 180.0),
            space: s.space,
            click_cycle_tolerance: s.click_cycle_tolerance.clamp(0.0, 50.0),
            debug,
            render,
            theme: s.theme,
            layout: s.layout,
            zoom_mode: s.zoom_mode,
            window_places: s.window_places.into_iter().filter(|(_, p)| [p.x, p.y, p.width, p.height].iter().all(|v| v.is_finite()) && p.width >= 1.0 && p.height >= 1.0).collect(),
            settings_order: s.settings_order,
            startup_notes,
            path,
            changed: false,
        }
    }

    fn to_saved(&self) -> Saved {
        Saved {
            shortcuts: self.keymap.overrides(),
            camera_speed: self.camera_speed,
            fast_multiplier: self.fast_multiplier,
            move_increment: self.move_increment,
            rotate_increment: self.rotate_increment,
            space: self.space,
            click_cycle_tolerance: self.click_cycle_tolerance,
            debug: self.debug.clone(),
            render: self.render,
            theme: self.theme.clone(),
            layout: self.layout.clone(),
            zoom_mode: self.zoom_mode,
            window_places: self.window_places.clone(),
            settings_order: self.settings_order,
        }
    }

    /// Remembers that something changed; [`EditorSettings::save_if_changed`] writes it out.
    pub fn touch(&mut self) {
        self.changed = true;
    }

    /// Writes the settings if they changed since the last save. Returns an error message to show.
    pub fn save_if_changed(&mut self) -> Option<String> {
        if !self.changed {
            return None;
        }
        self.changed = false;
        let path = self.path.as_ref()?;
        let result = (|| -> anyhow::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            write_atomic(path, serde_json::to_string_pretty(&self.to_saved())?)
        })();
        result.err().map(|e| format!("could not save the editor settings: {e:#}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{Action, Chord};

    fn temp_file(name: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("forge_settings_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        dir.join(name)
    }

    #[test]
    fn settings_round_trip_through_the_file() {
        let path = temp_file("editor_settings.json");
        let (mut s, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none(), "a missing file is just the defaults");
        s.keymap.add(Action::Save, Chord::parse("Ctrl+Alt+W").unwrap()).unwrap();
        s.camera_speed = 120.0;
        s.move_increment = 0.25;
        s.space = Space::Local;
        s.touch();
        assert_eq!(s.save_if_changed(), None);
        assert!(s.save_if_changed().is_none(), "nothing to write the second time");

        let (back, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none());
        assert_eq!(back.keymap.get(Action::Save), [Chord::parse("Ctrl+S").unwrap(), Chord::parse("Ctrl+Alt+W").unwrap()], "starter key kept, new key added");
        assert_eq!((back.camera_speed, back.move_increment, back.space), (120.0, 0.25, Space::Local));
        assert!(back.keymap.is_default(Action::Copy));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn the_colours_are_saved_and_a_bad_one_does_not_spoil_the_file() {
        use crate::theme::Slot;
        let path = temp_file("editor_settings.json");
        let (mut s, _) = EditorSettings::load_from(Some(path.clone()));
        assert!(s.theme.is_default(), "starts with the default colours");
        s.theme.set(Slot::Accent, [1, 2, 3]);
        s.camera_speed = 77.0;
        s.touch();
        assert_eq!(s.save_if_changed(), None);
        let (back, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none());
        assert_eq!(back.theme.get(Slot::Accent), [1, 2, 3]);
        assert_eq!(back.theme.get(Slot::Panel), Slot::Panel.default_rgb());

        std::fs::write(&path, r##"{ "camera_speed": 55, "theme": { "accent": "#0A0B0C", "panel": "pink", "outline": 7 } }"##).unwrap();
        let (s, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none(), "{problem:?}");
        assert_eq!((s.camera_speed, s.theme.get(Slot::Accent), s.theme.get(Slot::Panel), s.theme.get(Slot::Outline)), (55.0, [10, 11, 12], Slot::Panel.default_rgb(), Slot::Outline.default_rgb()));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_damaged_file_gives_the_defaults_and_a_message() {
        let path = temp_file("editor_settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let (s, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_some());
        assert_eq!(s.camera_speed, 40.0);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn values_from_a_file_are_kept_in_range_and_unknown_keys_are_ignored() {
        let path = temp_file("editor_settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{ "camera_speed": -5, "fast_multiplier": 1e9, "rotate_increment": 720, "from_the_future": true }"#).unwrap();
        let (s, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none());
        assert_eq!((s.camera_speed, s.fast_multiplier, s.rotate_increment), (0.1, 50.0, 180.0));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn the_second_speed_may_go_below_one_but_not_to_nothing() {
        let path = temp_file("editor_settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{ "fast_multiplier": 0.25 }"#).unwrap();
        let (s, _) = EditorSettings::load_from(Some(path.clone()));
        assert_eq!(s.fast_multiplier, 0.25, "slow, precise mode is allowed");
        std::fs::write(&path, r#"{ "fast_multiplier": 0.0 }"#).unwrap();
        let (s, _) = EditorSettings::load_from(Some(path.clone()));
        assert_eq!(s.fast_multiplier, MIN_SECOND_SPEED);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn the_viewport_grid_is_off_by_default_and_the_choice_is_kept_between_sessions() {
        let path = temp_file("editor_settings.json");
        let (mut s, _) = EditorSettings::load_from(Some(path.clone()));
        assert!(!s.render.show_grid, "a first start has no grid");
        s.render.show_grid = true;
        s.touch();
        assert_eq!(s.save_if_changed(), None);
        let (back, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none() && back.render.show_grid, "the next start has the grid on");

        // Turned off again, and a file from before the setting existed: no grid.
        let mut back = back;
        back.render.show_grid = false;
        back.touch();
        assert_eq!(back.save_if_changed(), None);
        assert!(!EditorSettings::load_from(Some(path.clone())).0.render.show_grid);
        std::fs::write(&path, r#"{ "render": { "vsync": "Off", "wireframe": true } }"#).unwrap();
        let (old, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none() && !old.render.show_grid && old.render.wireframe);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn the_zoom_mode_and_the_window_places_are_kept_and_bad_values_fall_back() {
        let path = temp_file("editor_settings.json");
        let (mut s, _) = EditorSettings::load_from(Some(path.clone()));
        assert_eq!(s.zoom_mode, ZoomMode::Camera, "camera-based is the default");
        assert!(s.window_places.is_empty());
        s.zoom_mode = ZoomMode::Hover;
        s.window_places.insert("editor_settings".into(), WindowPlace { x: 10.0, y: 20.0, width: 640.0, height: 480.0 });
        s.touch();
        assert_eq!(s.save_if_changed(), None);
        let (back, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none());
        assert_eq!(back.zoom_mode, ZoomMode::Hover);
        assert_eq!(back.window_places["editor_settings"], WindowPlace { x: 10.0, y: 20.0, width: 640.0, height: 480.0 });

        std::fs::write(&path, r#"{ "zoom_mode": "FromTheFuture", "window_places": { "a": { "x": 1, "y": 2, "width": 0, "height": 5 } } }"#).unwrap();
        let (s, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none(), "{problem:?}");
        assert_eq!(s.zoom_mode, ZoomMode::Camera, "an unknown mode reads as the default");
        assert!(s.window_places.is_empty(), "a place with no size is dropped");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_file_from_before_shortcuts_had_several_keys_still_loads() {
        let path = temp_file("editor_settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{ "shortcuts": { "general.save": "Ctrl+Alt+W", "general.undo": "", "general.copy": ["Ctrl+Insert", "Ctrl+C"] } }"#).unwrap();
        let (s, problem) = EditorSettings::load_from(Some(path.clone()));
        assert!(problem.is_none(), "{problem:?}");
        assert_eq!(s.keymap.get(Action::Save), [Chord::parse("Ctrl+Alt+W").unwrap()]);
        assert!(s.keymap.get(Action::Undo).is_empty(), "the old way of saying 'unbound'");
        assert_eq!(s.keymap.get(Action::Copy).len(), 2);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}