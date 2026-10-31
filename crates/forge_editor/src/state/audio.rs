//! Sound in the editor: the preview of a sound file (double-click a sound in the Asset Browser, or the play button in the
//! picker), and the mixer that plays the scene's `Sound` objects while a game plays in the viewport.
//!
//! Sounds do not play while the scene is only being edited. The preview uses the same mixer as the game, so what is heard
//! is what the game will play.

use forge_engine::{
    audio::{AudioEngine, SoundMirror, VoiceId, VoiceParams},
    project::file_name,
};

use super::Editor;

#[derive(Default)]
pub struct EditorAudio {
    pub engine: AudioEngine,
    pub mirror: SoundMirror,
    /// The file being previewed, and its voice.
    pub preview: Option<(String, VoiceId)>,
}

impl EditorAudio {
    /// Whether this file is being previewed right now.
    pub fn previewing(&self, path: &str) -> bool {
        self.preview.as_ref().is_some_and(|(p, v)| p == path && self.engine.mixer().is_playing(*v))
    }

    pub fn stop_preview(&mut self) {
        if let Some((_, voice)) = self.preview.take() {
            self.engine.mixer().stop(voice);
        }
    }
}

impl Editor {
    /// Plays a sound file once, to hear it. Playing it again stops it.
    pub fn preview_sound(&mut self, path: &str) {
        if self.audio.previewing(path) {
            self.audio.stop_preview();
            return;
        }
        self.audio.stop_preview();
        let Some(info) = self.dm.borrow().asset_info() else { return self.warn("Open a project first.") };
        match info.clip(path) {
            Ok(clip) => {
                self.audio.engine.prepare();
                if !self.audio.engine.has_device() {
                    let why = self.audio.engine.device_error.clone().unwrap_or_else(|| "no sound card".into());
                    self.warn(format!("{} cannot be heard: {why}", file_name(path)));
                }
                let voice = self.audio.engine.mixer().play(clip, 0.0, VoiceParams { volume: 0.8, ..VoiceParams::default() }, &[]);
                self.audio.preview = Some((path.to_owned(), voice));
            }
            Err(e) => self.warn(e),
        }
    }

    /// Once per frame: the mixer advances, the preview ends when it is over, and the scene's sounds are played while a
    /// game runs (and stopped otherwise).
    pub fn update_audio(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.25);
        self.audio.engine.update(dt);
        let running = self.play.is_some();
        {
            let audio = &mut self.audio;
            let mut dm = self.dm.borrow_mut();
            audio.mirror.update(&mut dm, &mut audio.engine, dt, running);
        }
        // (The mirror takes the events of the scene's own voices; a preview that ended just needs forgetting.)
        let ended = self.audio.preview.as_ref().is_some_and(|(_, v)| !self.audio.engine.mixer().is_playing(*v));
        if ended {
            self.audio.preview = None;
        }
    }
}