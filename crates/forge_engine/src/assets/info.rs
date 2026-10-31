//! What the data model can ask about the project's files without drawing or playing them: how big a model is, how long a
//! sound is, and the decoded sounds themselves.
//!
//! The answers are cached per file; [`AssetInfo::invalidate`] forgets them (the editor calls it when files change). A file
//! that cannot be read is remembered as an error, with the reason, so asking again is free and the message can be shown.

use std::{cell::{Cell, RefCell}, collections::HashMap, sync::Arc};

use bevy::math::Vec3;

use super::{mesh, source::GameSource};
use crate::audio::clip::{self, Clip};

/// Decoded sounds are kept up to this many bytes; past it, the ones nothing is playing are dropped.
const CLIP_BUDGET: usize = 256 * 1024 * 1024;

pub struct AssetInfo {
    source: GameSource,
    unit_scale: Cell<f32>,
    sizes: RefCell<HashMap<String, Result<Vec3, String>>>,
    clips: RefCell<HashMap<String, Result<Arc<Clip>, String>>>,
}

impl AssetInfo {
    pub fn new(source: GameSource) -> AssetInfo {
        AssetInfo { source, unit_scale: Cell::new(1.0), sizes: RefCell::default(), clips: RefCell::default() }
    }

    pub fn source(&self) -> &GameSource {
        &self.source
    }

    /// Studs per unit of a model file (a project setting).
    pub fn unit_scale(&self) -> f32 {
        self.unit_scale.get()
    }

    pub fn set_unit_scale(&self, scale: f32) {
        if scale.is_finite() && scale > 0.0 && scale != self.unit_scale.get() {
            self.unit_scale.set(scale);
            self.sizes.borrow_mut().clear();
        }
    }

    /// Forgets everything that was read (the files may have changed).
    pub fn invalidate(&self) {
        self.sizes.borrow_mut().clear();
        self.clips.borrow_mut().clear();
    }

    /// The size in studs of the model in a file (at the project's import scale), or why it cannot be had.
    pub fn mesh_size_result(&self, path: &str) -> Result<Vec3, String> {
        if path.is_empty() {
            return Err("no model is set".into());
        }
        if let Some(known) = self.sizes.borrow().get(path) {
            return known.clone();
        }
        let loaded = mesh::load(&self.source, path, self.unit_scale.get()).map(|m| m.size);
        self.sizes.borrow_mut().insert(path.to_owned(), loaded.clone());
        loaded
    }

    pub fn mesh_size(&self, path: &str) -> Option<Vec3> {
        self.mesh_size_result(path).ok()
    }

    /// The decoded sound in a file.
    pub fn clip(&self, path: &str) -> Result<Arc<Clip>, String> {
        if path.is_empty() {
            return Err("no sound is set".into());
        }
        if let Some(known) = self.clips.borrow().get(path) {
            return known.clone();
        }
        let decoded = self.source.read(path).map_err(|e| format!("{e:#}")).and_then(clip::decode).map(Arc::new).map_err(|e| format!("{path}: {e}"));
        let mut clips = self.clips.borrow_mut();
        let used: usize = clips.values().filter_map(|c| c.as_ref().ok()).map(|c| c.bytes()).sum();
        if used > CLIP_BUDGET {
            clips.retain(|_, c| c.as_ref().map_or(true, |c| Arc::strong_count(c) > 1));
        }
        clips.insert(path.to_owned(), decoded.clone());
        decoded
    }

    /// How long the sound in a file is, in seconds.
    pub fn audio_length(&self, path: &str) -> Option<f32> {
        self.clip(path).ok().map(|c| c.seconds())
    }

    /// Why a sound or model cannot be used, if it cannot (for the Properties panel and the logs).
    pub fn audio_problem(&self, path: &str) -> Option<String> {
        self.clip(path).err()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::clip::wav_bytes;

    fn project() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("forge_info_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(dir.join("Assets")).unwrap();
        dir
    }

    #[test]
    fn a_models_size_and_a_sounds_length_are_read_once_and_remembered() {
        let dir = project();
        std::fs::write(dir.join("Assets/box.obj"), "v 0 0 0\nv 3 0 0\nv 3 2 0\nv 0 2 0\nv 0 0 4\nv 3 0 4\nv 3 2 4\nv 0 2 4\nf 1 2 3 4\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n").unwrap();
        std::fs::write(dir.join("Assets/beep.wav"), wav_bytes(8000, 1, &vec![1000; 4000])).unwrap();
        let info = AssetInfo::new(GameSource::Dir(dir.clone()));
        assert_eq!(info.mesh_size("Assets/box.obj"), Some(Vec3::new(3.0, 2.0, 4.0)));
        assert!((info.audio_length("Assets/beep.wav").unwrap() - 0.5).abs() < 1e-3);
        // Remembered: the files can go and the answers stay until the cache is told to forget.
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(info.mesh_size("Assets/box.obj"), Some(Vec3::new(3.0, 2.0, 4.0)));
        assert!(info.clip("Assets/beep.wav").is_ok());
        info.invalidate();
        assert!(info.mesh_size("Assets/box.obj").is_none() && info.audio_length("Assets/beep.wav").is_none());
    }

    #[test]
    fn the_import_scale_changes_the_size_and_problems_come_with_a_reason() {
        let dir = project();
        std::fs::write(dir.join("Assets/box.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        let info = AssetInfo::new(GameSource::Dir(dir.clone()));
        assert_eq!(info.mesh_size("Assets/box.obj").unwrap().x, 1.0);
        info.set_unit_scale(2.5);
        assert_eq!(info.mesh_size("Assets/box.obj").unwrap().x, 2.5);
        info.set_unit_scale(f32::NAN);
        info.set_unit_scale(-1.0);
        assert_eq!(info.unit_scale(), 2.5, "a bad scale is ignored");
        assert!(info.mesh_size_result("").unwrap_err().contains("no model"));
        assert!(info.mesh_size_result("Assets/missing.obj").is_err());
        std::fs::write(dir.join("Assets/bad.wav"), b"not a sound").unwrap();
        assert!(info.audio_problem("Assets/bad.wav").is_some());
        assert!(info.audio_problem("Assets/missing.wav").is_some());
        assert!(info.audio_problem("").is_some());
        std::fs::remove_dir_all(dir).ok();
    }
}
