//! Choosing a file for a property or a material slot: the window that lists the project's files of the right kind
//! (`ui::asset_picker`), and what applying a choice does. Dragging a file from the Asset Browser onto the same fields
//! (`ui::asset_field`) ends in the same place, so the two ways of choosing behave alike.

use forge_engine::{
    core::Value,
    dm::Id,
    project::{file_name, AssetKind},
};

use super::Editor;

/// What a choice is for.
#[derive(Clone, Debug, PartialEq)]
pub enum PickTarget {
    /// A property of these objects (`MeshId`, `SoundId`, `ColorMap`, `MaterialVariant` ...).
    Property { targets: Vec<Id>, prop: &'static str },
    /// A texture slot of the material in the Material Editor (`base_color`, `normal` ...).
    MaterialMap(&'static str),
}

/// The open picker window.
pub struct AssetPicker {
    pub target: PickTarget,
    pub kind: AssetKind,
    pub title: String,
    pub search: String,
    /// The file the field has now (shown selected).
    pub current: String,
    /// The window was put on the screen (it is centred the first time).
    pub placed: bool,
}

impl Editor {
    pub fn open_picker(&mut self, target: PickTarget, kind: AssetKind, title: impl Into<String>, current: &str) {
        self.sync_assets(false);
        self.picker = Some(AssetPicker { target, kind, title: title.into(), search: String::new(), current: current.to_owned(), placed: false });
    }

    /// The project's files of a kind that the engine can use, that match `search` (words of the name or path), by path.
    pub fn pickable_assets(&self, kind: AssetKind, search: &str) -> Vec<String> {
        let Some(db) = self.asset_db() else { return Vec::new() };
        let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
        db.entries()
            .iter()
            .filter(|e| e.kind == kind && AssetKind::usable_by_engine(e.name()))
            .filter(|e| {
                let lower = e.path.to_lowercase();
                words.iter().all(|w| lower.contains(w.as_str()))
            })
            .map(|e| e.path.clone())
            .collect()
    }

    /// The kind of file a property of this asset type takes (`Asset("ogg")` takes sounds, `Asset("obj")` models ...).
    pub fn kind_of_asset_ext(ext: &str) -> AssetKind {
        AssetKind::of(&format!("x.{ext}"))
    }

    /// A choice was made (from the picker, or by dropping a file on a field): `path` is the new value (empty: none).
    pub fn apply_asset(&mut self, target: &PickTarget, path: &str) {
        match target {
            PickTarget::Property { targets, prop } => self.assign_asset_prop(targets, prop, path),
            PickTarget::MaterialMap(slot) => self.set_material_map(slot, path),
        }
    }

    /// Writes a file path to a property of every target, undoably, and reports what could not be done.
    pub fn assign_asset_prop(&mut self, targets: &[Id], prop: &'static str, path: &str) {
        let mut done = 0;
        let mut failure = None;
        {
            let mut dm = self.dm.borrow_mut();
            for target in targets {
                if !dm.exists(*target) {
                    continue;
                }
                let first_mesh = prop == "MeshId" && dm.string(*target, "MeshId").is_empty();
                match dm.set_prop(*target, prop, Value::Str(path.to_owned())) {
                    Ok(()) => {
                        done += 1;
                        // The first model a part gets gives it the model's own size.
                        if first_mesh && !path.is_empty() {
                            if let Some(size) = dm.assets().and_then(|a| a.mesh_size(path)) {
                                let _ = dm.set_prop(*target, "Size", Value::V3(size));
                            }
                        }
                    }
                    Err(e) => failure = Some(e),
                }
            }
        }
        if let Some(e) = failure {
            self.warn(e);
        }
        if done > 0 {
            self.dirty = true;
            self.info(if path.is_empty() { format!("Cleared {prop} on {done} object(s)") } else { format!("{prop} is now {} on {done} object(s)", file_name(path)) });
        }
    }
}