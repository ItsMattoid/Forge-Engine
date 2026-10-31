//! Runtime player settings: what the person playing a built game chose for themselves.
//!
//! The project's own settings (the `[window]` table of `Project.toml`) are the *defaults* the developer ships: they
//! are what a player gets the first time. A player can change them for their own computer - a smaller window, no
//! V-Sync, lower shadows - without the game being rebuilt; that choice is kept per game, per user, in
//! `Forge/Players/<game>/player_settings.json` in the user's config folder (on Windows `%APPDATA%`), and wins over the
//! project's settings whenever the game starts. Every field is optional: a missing one keeps the project's default.
//! None of this is read by the editor, whose viewport has settings of its own (Editor Settings > Rendering).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{DisplayMode, QualityPreset, ShadowQuality, WindowSettings};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerSettings {
    pub display_mode: Option<DisplayMode>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub vsync: Option<bool>,
    /// 0 = no limit.
    pub fps_limit: Option<u32>,
    /// A preset sets the anti-aliasing and the shadows; the two settings below then refine it.
    pub quality: Option<String>,
    pub msaa: Option<u8>,
    pub shadow_quality: Option<ShadowQuality>,
}

impl PlayerSettings {
    /// Where the settings of the game with this slug are kept (`None` when the system has no config folder).
    pub fn path_for(slug: &str) -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Forge").join("Players").join(slug).join("player_settings.json"))
    }

    /// The player's settings for this game, if there are any. A file that cannot be read is ignored (the game
    /// starts with the project's defaults rather than not at all).
    pub fn load(slug: &str) -> Option<PlayerSettings> {
        Self::load_from(&Self::path_for(slug)?)
    }

    pub fn load_from(path: &std::path::Path) -> Option<PlayerSettings> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    pub fn save_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        super::write_atomic(path, serde_json::to_string_pretty(self)?)
    }

    /// Puts the player's choices over the project's defaults, then keeps everything in range.
    pub fn apply_to(&self, window: &mut WindowSettings) {
        if let Some(preset) = self.quality.as_deref().and_then(|name| QualityPreset::CHOICES.into_iter().find(|p| p.label().eq_ignore_ascii_case(name))) {
            window.apply_quality(preset);
        }
        if let Some(v) = self.msaa {
            window.msaa = v;
        }
        if let Some(v) = self.shadow_quality {
            window.shadow_quality = v;
        }
        if let Some(mode) = self.display_mode {
            window.set_display_mode(mode);
        }
        if let Some(v) = self.width {
            window.width = v;
        }
        if let Some(v) = self.height {
            window.height = v;
        }
        if let Some(v) = self.vsync {
            window.vsync = v;
        }
        if let Some(v) = self.fps_limit {
            window.fps_limit = v;
        }
        window.sanitize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_player_chose_wins_and_the_rest_stays_the_projects() {
        let mut window = WindowSettings { width: 1920, height: 1080, vsync: true, ..WindowSettings::default() };
        let player = PlayerSettings { vsync: Some(false), fps_limit: Some(144), quality: Some("low".into()), display_mode: Some(DisplayMode::Fullscreen), ..PlayerSettings::default() };
        player.apply_to(&mut window);
        assert!(!window.vsync && window.fps_limit == 144 && window.fullscreen && window.display_mode == DisplayMode::Fullscreen);
        assert_eq!((window.msaa, window.shadow_quality, window.quality()), (0, ShadowQuality::Low, QualityPreset::Low));
        assert_eq!((window.width, window.height), (1920, 1080), "not chosen: the project's");
        // A refinement after the preset, and nonsense held in range.
        let finer = PlayerSettings { quality: Some("Ultra".into()), msaa: Some(2), width: Some(5), fps_limit: Some(1), ..PlayerSettings::default() };
        finer.apply_to(&mut window);
        assert_eq!((window.msaa, window.shadow_quality, window.quality()), (2, ShadowQuality::High, QualityPreset::Custom));
        assert_eq!((window.width, window.fps_limit), (160, 10));
        PlayerSettings::default().apply_to(&mut window);
        assert_eq!(window.msaa, 2, "nothing chosen changes nothing");
    }

    #[test]
    fn the_file_round_trips_and_a_broken_one_is_ignored() {
        let dir = std::env::temp_dir().join(format!("forge_player_settings_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let path = dir.join("nested").join("player_settings.json");
        assert_eq!(PlayerSettings::load_from(&path), None, "no file: no choices");
        let settings = PlayerSettings { vsync: Some(false), shadow_quality: Some(ShadowQuality::Off), ..PlayerSettings::default() };
        settings.save_to(&path).unwrap();
        assert_eq!(PlayerSettings::load_from(&path), Some(settings));
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(PlayerSettings::load_from(&path), None);
        std::fs::write(&path, r#"{ "vsync": false, "from_the_future": 1 }"#).unwrap();
        assert_eq!(PlayerSettings::load_from(&path).unwrap().vsync, Some(false), "unknown keys are ignored");
        std::fs::remove_dir_all(&dir).ok();
    }
}
