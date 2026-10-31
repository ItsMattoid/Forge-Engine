//! The `Project.toml` manifest (`forge.toml` in older projects): project info, game type, window and graphics
//! settings, and the launch presentation (icon and splash screen).
//!
//! ```toml
//! [project]            name, version, start scene
//! [game]               type = "single_player" | "multiplayer"
//! [multiplayer]        reserved for networking options; kept as written, whatever the game type
//! [window]             title, size, display mode, V-Sync, frame-rate limit, anti-aliasing, shadows ...
//! [launch]             icon and splash screen: project files, referred to by their path in the project
//! ```
//!
//! Everything has a default and unknown keys are kept, so a manifest from an older or a newer Forge loads, and
//! the editor saving it loses nothing.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Keys of the manifest this version of Forge does not know. They are kept and written back,
/// so a manifest edited by hand (or by a newer Forge) does not lose anything when the editor saves it.
type Extra = std::collections::BTreeMap<String, toml::Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectInfo {
    pub name: String,
    pub version: String,
    pub start_scene: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for ProjectInfo {
    fn default() -> Self {
        Self { name: "Untitled Game".into(), version: "0.1.0".into(), start_scene: "Scenes/MainScene.scene".into(), extra: Extra::new() }
    }
}

// ---- game type -----------------------------------------------------------------------------------------

/// What kind of game the project is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameType {
    /// Several players. Forge has no networking yet, so for now this records what the game is meant to be; the
    /// options that belong to it (see [`MultiplayerSettings`]) are added as the engine gets them.
    Multiplayer,
    /// One player on one computer. (A name this version does not know also reads as this.)
    #[default]
    #[serde(other)]
    SinglePlayer,
}

impl GameType {
    pub const ALL: [GameType; 2] = [GameType::SinglePlayer, GameType::Multiplayer];

    pub fn label(self) -> &'static str {
        match self {
            GameType::SinglePlayer => "Single-player",
            GameType::Multiplayer => "Multiplayer",
        }
    }

    pub fn is_multiplayer(self) -> bool {
        self == GameType::Multiplayer
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameSettings {
    #[serde(rename = "type")]
    pub kind: GameType,
    #[serde(flatten)]
    pub extra: Extra,
}

/// The settings that only a multiplayer game has. There are none yet that the engine supports (no networking, one
/// local player), so this holds nothing but what an older or newer manifest wrote: it is kept as it is when the game
/// type is changed back and forth, and is where a new option goes (a field here, a row in the editor's Multiplayer
/// page) once the engine can honour it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiplayerSettings {
    #[serde(flatten)]
    pub extra: Extra,
}

impl MultiplayerSettings {
    pub fn is_empty(&self) -> bool {
        self.extra.is_empty()
    }
}

// ---- assets -----------------------------------------------------------------------------------------

/// How the project's files are read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetSettings {
    /// Studs per unit of a model file: a model made 2 units tall is `2 * mesh_unit_scale` studs tall. 1 means a unit in
    /// the file is a stud (what most modelling tools export when told "1 unit = 1 stud"); 3.571 reads a file made in metres
    /// as Roblox-sized studs (0.28 m each).
    pub mesh_unit_scale: f32,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for AssetSettings {
    fn default() -> Self {
        AssetSettings { mesh_unit_scale: 1.0, extra: Extra::new() }
    }
}

impl AssetSettings {
    pub fn is_default(&self) -> bool {
        *self == AssetSettings::default()
    }

    /// The scale, or the default when the file holds nonsense (zero, negative, not a number).
    pub fn mesh_scale(&self) -> f32 {
        if self.mesh_unit_scale.is_finite() && self.mesh_unit_scale > 0.0 {
            self.mesh_unit_scale
        } else {
            1.0
        }
    }
}

// ---- window and graphics -------------------------------------------------------------------------------

/// How the game's window is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// A borderless window covering the whole screen: switching to another program is instant.
    Borderless,
    /// The game takes over the screen (at the screen's own resolution): the usual "fullscreen".
    Fullscreen,
    /// A window of the configured size. (A name this version does not know also reads as this.)
    #[default]
    #[serde(other)]
    Windowed,
}

impl DisplayMode {
    pub const ALL: [DisplayMode; 3] = [DisplayMode::Windowed, DisplayMode::Borderless, DisplayMode::Fullscreen];

    pub fn label(self) -> &'static str {
        match self {
            DisplayMode::Windowed => "Windowed",
            DisplayMode::Borderless => "Borderless fullscreen",
            DisplayMode::Fullscreen => "Fullscreen",
        }
    }
}

/// How good the shadows of the sun and the lights look (and how much they cost): the size of the shadow maps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowQuality {
    /// No shadows at all, whatever the lights ask for.
    Off,
    Low,
    High,
    /// What Forge always drew. (A name this version does not know also reads as this.)
    #[default]
    #[serde(other)]
    Medium,
}

impl ShadowQuality {
    pub const ALL: [ShadowQuality; 4] = [ShadowQuality::Off, ShadowQuality::Low, ShadowQuality::Medium, ShadowQuality::High];

    pub fn label(self) -> &'static str {
        match self {
            ShadowQuality::Off => "Off",
            ShadowQuality::Low => "Low",
            ShadowQuality::Medium => "Medium",
            ShadowQuality::High => "High",
        }
    }

    /// The side of the shadow maps in pixels (0 when there are no shadows).
    pub fn map_size(self) -> usize {
        match self {
            ShadowQuality::Off => 0,
            ShadowQuality::Low => 1024,
            ShadowQuality::Medium => 2048,
            ShadowQuality::High => 4096,
        }
    }
}

/// A named set of the quality settings (anti-aliasing and shadows). Not stored: it is read off the settings, so it
/// can never disagree with them - [`QualityPreset::Custom`] is what any other combination is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityPreset {
    Low,
    Medium,
    High,
    Ultra,
    Custom,
}

impl QualityPreset {
    /// The presets that can be chosen (Custom is what editing the individual settings gives).
    pub const CHOICES: [QualityPreset; 4] = [QualityPreset::Low, QualityPreset::Medium, QualityPreset::High, QualityPreset::Ultra];

    pub fn label(self) -> &'static str {
        match self {
            QualityPreset::Low => "Low",
            QualityPreset::Medium => "Medium",
            QualityPreset::High => "High",
            QualityPreset::Ultra => "Ultra",
            QualityPreset::Custom => "Custom",
        }
    }

    /// Anti-aliasing samples and shadow quality of a preset (`None` for Custom).
    pub fn settings(self) -> Option<(u8, ShadowQuality)> {
        match self {
            QualityPreset::Low => Some((0, ShadowQuality::Low)),
            QualityPreset::Medium => Some((2, ShadowQuality::Medium)),
            // (What Forge always did: 4x anti-aliasing and 2048-pixel shadow maps.)
            QualityPreset::High => Some((4, ShadowQuality::Medium)),
            QualityPreset::Ultra => Some((8, ShadowQuality::High)),
            QualityPreset::Custom => None,
        }
    }
}

/// The frame-rate limits the settings accept (0 = no limit).
pub const MIN_FPS_LIMIT: u32 = 10;
pub const MAX_FPS_LIMIT: u32 = 1000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    /// Window title; falls back to the project name.
    pub title: Option<String>,
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub resizable: bool,
    /// Older manifests say only whether the game is fullscreen; it is kept in step with `display_mode`
    /// (anything but windowed) so such a file still means the same to a Forge that does not know the mode.
    pub fullscreen: bool,
    /// How the window is shown. A manifest that only has `fullscreen = true` gets a borderless window.
    pub display_mode: DisplayMode,
    /// MSAA samples: 0 (off), 2, 4 or 8.
    pub msaa: u8,
    /// The game never draws more frames per second than this (0 = no limit).
    pub fps_limit: u32,
    /// The size of the shadow maps, or no shadows.
    pub shadow_quality: ShadowQuality,
    /// Skip drawing what is hidden behind other geometry, on graphics hardware that can do it. Off by
    /// default: it only pays off in scenes limited by the GPU (heavy hidden overdraw) without MSAA, and
    /// costs frame time elsewhere (see `render::culling` for the measurements).
    pub occlusion_culling: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            title: None,
            width: 1280,
            height: 720,
            vsync: true,
            resizable: true,
            fullscreen: false,
            display_mode: DisplayMode::Windowed,
            msaa: 4,
            fps_limit: 0,
            shadow_quality: ShadowQuality::Medium,
            occlusion_culling: false,
            extra: Extra::new(),
        }
    }
}

impl WindowSettings {
    /// The quality preset the anti-aliasing and shadow settings add up to (Custom if they are none of them).
    pub fn quality(&self) -> QualityPreset {
        QualityPreset::CHOICES.into_iter().find(|p| p.settings() == Some((self.msaa, self.shadow_quality))).unwrap_or(QualityPreset::Custom)
    }

    /// Sets the settings a preset stands for (Custom changes nothing).
    pub fn apply_quality(&mut self, preset: QualityPreset) {
        if let Some((msaa, shadows)) = preset.settings() {
            (self.msaa, self.shadow_quality) = (msaa, shadows);
        }
    }

    /// Chooses how the window is shown, keeping the older `fullscreen` flag in step.
    pub fn set_display_mode(&mut self, mode: DisplayMode) {
        self.display_mode = mode;
        self.fullscreen = mode != DisplayMode::Windowed;
    }

    /// Puts the values in range and makes the old and the new way of saying "fullscreen" agree.
    pub fn sanitize(&mut self) {
        self.width = self.width.clamp(160, 16_384);
        self.height = self.height.clamp(120, 16_384);
        if !matches!(self.msaa, 0 | 2 | 4 | 8) {
            self.msaa = 4;
        }
        if self.fps_limit != 0 {
            self.fps_limit = self.fps_limit.clamp(MIN_FPS_LIMIT, MAX_FPS_LIMIT);
        }
        // A manifest from before display modes: `fullscreen = true` meant a borderless window.
        if self.fullscreen && self.display_mode == DisplayMode::Windowed {
            self.display_mode = DisplayMode::Borderless;
        }
        self.fullscreen = self.display_mode != DisplayMode::Windowed;
    }
}

// ---- launch presentation -------------------------------------------------------------------------------

/// How the game presents itself while it starts. Both are files of the project, named by their path inside it (so
/// they are packed with the game and follow the file when it is moved or renamed in the editor), never by a path on
/// the developer's computer. They are separate: the icon is the picture of the game's window (and the program), the
/// splash screen is shown over the window while the game loads.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchSettings {
    /// The application icon: a square PNG or JPEG.
    pub icon: Option<String>,
    /// An optional picture shown while the game launches: a PNG or JPEG.
    pub splash: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl LaunchSettings {
    pub fn is_empty(&self) -> bool {
        self.icon.is_none() && self.splash.is_none() && self.extra.is_empty()
    }

    /// Every project file the launch settings refer to.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.icon.iter().chain(self.splash.iter()).map(String::as_str)
    }
}

// ---- the manifest --------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectManifest {
    pub project: ProjectInfo,
    pub game: GameSettings,
    /// How the project's files are read (only written when it differs from the default).
    #[serde(skip_serializing_if = "AssetSettings::is_default")]
    pub assets: AssetSettings,
    #[serde(skip_serializing_if = "MultiplayerSettings::is_empty")]
    pub multiplayer: MultiplayerSettings,
    pub window: WindowSettings,
    #[serde(skip_serializing_if = "LaunchSettings::is_empty")]
    pub launch: LaunchSettings,
    #[serde(flatten)]
    pub extra: Extra,
}

impl ProjectManifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes).context("the project manifest is not valid UTF-8")?;
        toml::from_str::<Self>(text.trim_start_matches('\u{feff}')).context("invalid project manifest").map(Self::sanitized)
    }

    pub fn window_title(&self) -> &str {
        self.window.title.as_deref().unwrap_or(&self.project.name)
    }

    pub fn game_type(&self) -> GameType {
        self.game.kind
    }

    /// A manifest from outside is not trusted: values that would crash the window or the
    /// renderer are replaced by sane ones.
    pub fn sanitized(mut self) -> Self {
        self.window.sanitize();
        // A file reference is a plain project-relative path or nothing.
        for file in [&mut self.launch.icon, &mut self.launch.splash] {
            *file = file.take().map(|f| f.trim().replace('\\', "/")).filter(|f| !f.is_empty());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trip() {
        let m = ProjectManifest::default();
        let text = toml::to_string_pretty(&m).unwrap();
        assert_eq!(ProjectManifest::parse(text.as_bytes()).unwrap(), m);
    }

    #[test]
    fn unknown_keys_survive_a_save() {
        let text = "schema = 2\n\n[project]\nname = \"Kept\"\nauthor = \"me\"\n\n[window]\nwidth = 800\nicon = \"icon.png\"\n\n[dependencies]\nfoo = \"1\"\n";
        let m = ProjectManifest::parse(text.as_bytes()).unwrap();
        let saved = toml::to_string_pretty(&m).unwrap();
        let again = ProjectManifest::parse(saved.as_bytes()).unwrap();
        assert_eq!(again.project.extra.get("author"), Some(&toml::Value::String("me".into())));
        assert_eq!(again.window.extra.get("icon"), Some(&toml::Value::String("icon.png".into())));
        assert!(again.extra.contains_key("dependencies"));
        assert_eq!(again.extra.get("schema"), Some(&toml::Value::Integer(2)));
        assert_eq!(again.window.width, 800);
    }

    #[test]
    fn occlusion_culling_is_off_unless_the_project_turns_it_on() {
        assert!(!ProjectManifest::parse(b"[window]\nwidth = 800\n").unwrap().window.occlusion_culling, "a manifest from before the setting existed gets the default");
        let on = ProjectManifest::parse(b"[window]\nocclusion_culling = true\n").unwrap();
        assert!(on.window.occlusion_culling);
        let saved = toml::to_string_pretty(&on).unwrap();
        assert!(ProjectManifest::parse(saved.as_bytes()).unwrap().window.occlusion_culling, "and it is saved");
    }

    #[test]
    fn nonsense_window_settings_are_corrected() {
        let m = ProjectManifest::parse(b"[window]\nwidth = 0\nheight = 4000000000\nmsaa = 3\nfps_limit = 3\n").unwrap();
        assert_eq!((m.window.width, m.window.height, m.window.msaa, m.window.fps_limit), (160, 16_384, 4, 10));
        assert_eq!(ProjectManifest::parse(b"[window]\nfps_limit = 99999\n").unwrap().window.fps_limit, MAX_FPS_LIMIT);
        assert_eq!(ProjectManifest::parse(b"[window]\nfps_limit = 0\n").unwrap().window.fps_limit, 0, "0 is no limit");
    }

    // ---- game type ----

    #[test]
    fn a_manifest_without_a_game_section_is_a_single_player_game_and_the_type_is_saved() {
        let old = ProjectManifest::parse(b"[project]\nname = \"Old\"\n").unwrap();
        assert_eq!(old.game_type(), GameType::SinglePlayer);
        let mut m = old;
        m.game.kind = GameType::Multiplayer;
        let text = toml::to_string_pretty(&m).unwrap();
        assert!(text.contains("type = \"multiplayer\""), "{text}");
        assert_eq!(ProjectManifest::parse(text.as_bytes()).unwrap().game_type(), GameType::Multiplayer);
        assert_eq!(ProjectManifest::parse(b"[game]\ntype = \"from_the_future\"\n").unwrap().game_type(), GameType::SinglePlayer, "an unknown type reads as the default");
    }

    #[test]
    fn multiplayer_settings_are_kept_whatever_the_game_type_is() {
        let text = "[game]\ntype = \"multiplayer\"\n\n[multiplayer]\nslots = 8\nregion = \"eu\"\n";
        let mut m = ProjectManifest::parse(text.as_bytes()).unwrap();
        assert_eq!(m.multiplayer.extra.get("slots"), Some(&toml::Value::Integer(8)));
        // Single-player and back: nothing is dropped from the file.
        m.game.kind = GameType::SinglePlayer;
        let saved = toml::to_string_pretty(&m).unwrap();
        let single = ProjectManifest::parse(saved.as_bytes()).unwrap();
        assert_eq!(single.game_type(), GameType::SinglePlayer);
        assert_eq!(single.multiplayer.extra.get("region"), Some(&toml::Value::String("eu".into())), "the multiplayer settings stay in the file");
        let mut back = single;
        back.game.kind = GameType::Multiplayer;
        let again = ProjectManifest::parse(toml::to_string_pretty(&back).unwrap().as_bytes()).unwrap();
        assert_eq!((again.game_type(), again.multiplayer.extra.len()), (GameType::Multiplayer, 2));
        // A project that never had any does not get an empty section.
        assert!(!toml::to_string_pretty(&ProjectManifest::default()).unwrap().contains("[multiplayer]"));
    }

    // ---- graphics ----

    #[test]
    fn display_modes_agree_with_the_older_fullscreen_flag() {
        let old = ProjectManifest::parse(b"[window]\nfullscreen = true\n").unwrap();
        assert_eq!(old.window.display_mode, DisplayMode::Borderless, "fullscreen before display modes meant a borderless window");
        let mut w = WindowSettings::default();
        w.set_display_mode(DisplayMode::Fullscreen);
        assert!(w.fullscreen, "an older Forge reading the file still sees fullscreen");
        w.set_display_mode(DisplayMode::Windowed);
        assert!(!w.fullscreen);
        let m = ProjectManifest::parse(b"[window]\ndisplay_mode = \"fullscreen\"\n").unwrap();
        assert!(m.window.fullscreen && m.window.display_mode == DisplayMode::Fullscreen);
        assert_eq!(ProjectManifest::parse(b"[window]\ndisplay_mode = \"banana\"\n").unwrap().window.display_mode, DisplayMode::Windowed);
        let saved = toml::to_string_pretty(&m).unwrap();
        assert_eq!(ProjectManifest::parse(saved.as_bytes()).unwrap().window.display_mode, DisplayMode::Fullscreen);
    }

    #[test]
    fn quality_presets_are_read_off_the_settings_and_a_mixture_is_custom() {
        let mut w = WindowSettings::default();
        assert_eq!(w.quality(), QualityPreset::High, "a new project has what Forge always drew");
        assert_eq!((w.msaa, w.shadow_quality), (4, ShadowQuality::Medium));
        for preset in QualityPreset::CHOICES {
            w.apply_quality(preset);
            assert_eq!(w.quality(), preset);
        }
        w.apply_quality(QualityPreset::Low);
        w.shadow_quality = ShadowQuality::High;
        assert_eq!(w.quality(), QualityPreset::Custom, "changing one setting leaves the presets");
        let before = (w.msaa, w.shadow_quality);
        w.apply_quality(QualityPreset::Custom);
        assert_eq!((w.msaa, w.shadow_quality), before, "Custom changes nothing");
        assert_eq!(ShadowQuality::Off.map_size(), 0);
        assert!(ShadowQuality::Low.map_size() < ShadowQuality::Medium.map_size() && ShadowQuality::Medium.map_size() < ShadowQuality::High.map_size());
    }

    // ---- launch presentation ----

    #[test]
    fn the_icon_and_the_splash_are_separate_project_paths_that_are_saved() {
        let m = ProjectManifest::parse(b"[launch]\nicon = \"Assets\\\\Textures\\\\icon.png\"\nsplash = \"  \"\n").unwrap();
        assert_eq!(m.launch.icon.as_deref(), Some("Assets/Textures/icon.png"), "one spelling of a path");
        assert_eq!(m.launch.splash, None, "a blank one is none");
        let both = ProjectManifest { launch: LaunchSettings { icon: Some("a.png".into()), splash: Some("b.jpg".into()), ..LaunchSettings::default() }, ..ProjectManifest::default() };
        let back = ProjectManifest::parse(toml::to_string_pretty(&both).unwrap().as_bytes()).unwrap();
        assert_eq!((back.launch.icon.as_deref(), back.launch.splash.as_deref()), (Some("a.png"), Some("b.jpg")));
        assert_eq!(back.launch.files().collect::<Vec<_>>(), ["a.png", "b.jpg"]);
        assert!(!toml::to_string_pretty(&ProjectManifest::default()).unwrap().contains("[launch]"), "nothing chosen, nothing written");
    }
}
