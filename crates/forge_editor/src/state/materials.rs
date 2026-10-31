//! The Material Editor's state, and what editing a material does.
//!
//! A material is a `.mat` file of the project (see `forge_engine::assets::material`). The editor edits one at a time: the
//! window changes `MaterialEditor::def`, and [`Editor::material_changed`] writes the file as soon as it differs from what
//! was last saved, so the viewport - which draws parts from the files - shows every change at once. The materials the
//! project starts with are ordinary files here: any of them can be edited, replaced, duplicated or deleted, and new ones
//! are created without touching the engine.

use std::collections::HashMap;

use forge_engine::{
    assets::material::{MaterialDef, MaterialKind, MATERIALS_DIR, MATERIAL_EXT},
    project::{file_name, join, parent_of, stem_of, write_atomic, AssetKind},
};

use super::Editor;

/// The Material Editor window.
pub struct MaterialEditor {
    pub open: bool,
    /// The window was put somewhere (it is placed the first time it opens).
    pub placed: bool,
    /// The material being edited.
    pub path: Option<String>,
    pub def: MaterialDef,
    /// The file as last read or written: `def` differing from it is an unsaved change.
    pub saved: MaterialDef,
    /// Why the file could not be read, or what is wrong with the material (missing pictures ...).
    pub problems: Vec<String>,
    pub search: String,
    /// The name being typed for a rename.
    pub renaming: Option<String>,
    /// The editor's copy differs from the file and has not been written yet (writes wait while a slider is dragged).
    pub pending: bool,
    pub last_save: Option<std::time::Instant>,
    /// The preview picture and what it was made from.
    pub preview: Option<(u64, bevy_egui::egui::TextureHandle)>,
    /// Pictures decoded for the preview, by path (None: could not be read).
    pub preview_textures: HashMap<String, Option<std::sync::Arc<image::RgbaImage>>>,
}

impl Default for MaterialEditor {
    fn default() -> Self {
        MaterialEditor {
            open: false,
            placed: false,
            path: None,
            def: MaterialDef::default(),
            saved: MaterialDef::default(),
            problems: Vec::new(),
            search: String::new(),
            renaming: None,
            pending: false,
            last_save: None,
            preview: None,
            preview_textures: HashMap::new(),
        }
    }
}

impl Editor {
    /// Every material file of the project.
    pub fn material_paths(&self) -> Vec<String> {
        self.asset_db().map(|db| db.entries().iter().filter(|e| e.kind == AssetKind::Material).map(|e| e.path.clone()).collect()).unwrap_or_default()
    }

    /// Opens the Material Editor, on a material if one is given (else on the one it had, or the first).
    pub fn open_material_editor(&mut self, path: Option<&str>) {
        if self.project.is_none() {
            return self.warn("Open a project first.");
        }
        self.sync_assets(false);
        self.material_editor.open = true;
        match path {
            Some(p) => self.select_material(p),
            None => {
                let keep = self.material_editor.path.clone().filter(|p| self.asset_db().is_some_and(|db| db.entry(p).is_some()));
                match keep.or_else(|| self.material_paths().into_iter().next()) {
                    Some(p) => self.select_material(&p),
                    None => self.material_editor.path = None,
                }
            }
        }
    }

    pub fn close_material_editor(&mut self) {
        self.material_editor.open = false;
    }

    /// Reads a material file into the editor.
    pub fn select_material(&mut self, path: &str) {
        self.flush_material(false, true);
        let Some(project) = self.project.as_ref() else { return };
        self.material_editor.renaming = None;
        self.material_editor.path = Some(path.to_owned());
        let read = project.abs(path).map_err(|e| format!("{e:#}")).and_then(|p| std::fs::read(p).map_err(|e| format!("{e}"))).and_then(|b| MaterialDef::parse(&b));
        match read {
            Ok((def, notes)) => {
                self.material_editor.problems = notes;
                self.material_editor.saved = def.clone();
                self.material_editor.def = def;
                self.check_material();
            }
            Err(e) => {
                self.material_editor.def = MaterialDef::default();
                self.material_editor.saved = MaterialDef::default();
                self.material_editor.problems = vec![format!("{path} cannot be read: {e}")];
            }
        }
    }

    /// Looks for what is wrong with the material being edited besides its numbers: pictures that are missing or that Forge cannot read.
    pub fn check_material(&mut self) {
        let mut problems: Vec<String> = Vec::new();
        let def = self.material_editor.def.clone();
        for (slot, path) in def.maps.set() {
            let name = file_name(path);
            match self.asset_db().and_then(|db| db.entry(path)) {
                None if !path.contains('#') => problems.push(format!("The {slot} map {path} is not in the project.")),
                _ if !AssetKind::usable_by_engine(name) && !path.contains('#') => problems.push(format!("The {slot} map: {}", AssetKind::unusable_reason(name))),
                _ => {}
            }
        }
        if def.uv.mode == forge_engine::assets::material::UvMode::Studs && def.alpha_mode == forge_engine::assets::material::AlphaKind::Mask {
            problems.push("A cut-out (mask) material uses the model's own texture coordinates: Bevy's depth and shadow passes only know those.".to_owned());
        }
        if def.kind == MaterialKind::Glass && def.opacity >= 0.99 && def.transmission <= 0.0 {
            problems.push("A glass with full opacity looks like plain plastic: lower Opacity.".to_owned());
        }
        // (Keep the notes from reading the file; the rest is recomputed.)
        let notes: Vec<String> = self.material_editor.problems.iter().filter(|p| p.contains("outside") || p.contains("unusable") || p.contains("cannot be read")).cloned().collect();
        self.material_editor.problems = notes.into_iter().chain(problems).collect();
    }

    /// The material was edited: it is written to its file soon (see [Editor::flush_material]).
    pub fn material_changed(&mut self) {
        if self.material_editor.path.is_none() || self.material_editor.def == self.material_editor.saved {
            return;
        }
        self.material_editor.def.sanitize();
        self.material_editor.pending = true;
    }

    /// Writes an edited material to its file. While a slider is being dragged the file is written at most every 150 ms (every
    /// write makes every part look at the material again); once the mouse is let go, at once. orce writes now.
    pub fn flush_material(&mut self, dragging: bool, force: bool) {
        if !self.material_editor.pending {
            return;
        }
        let waiting = self.material_editor.last_save.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(150));
        if !force && dragging && waiting {
            return;
        }
        self.material_editor.pending = false;
        let Some(path) = self.material_editor.path.clone() else { return };
        self.check_material();
        self.save_material(&path);
        self.material_editor.last_save = Some(std::time::Instant::now());
    }

    fn save_material(&mut self, path: &str) {
        let Some(project) = self.project.as_ref() else { return };
        let text = self.material_editor.def.to_json();
        let result = project.abs(path).map_err(|e| format!("{e:#}")).and_then(|p| write_atomic(&p, text).map_err(|e| format!("{e:#}")));
        match result {
            Ok(()) => {
                self.material_editor.saved = self.material_editor.def.clone();
                self.own_writes.insert(path.to_owned());
                // The viewport draws from the file: it reads it again and every part uses the new look.
                self.render_assets_changed();
            }
            Err(e) => self.error(format!("could not save {path}: {e}")),
        }
    }

    /// A new material file in `Assets/Materials`, starting as `kind` looks by default. Returns its path.
    pub fn new_material(&mut self, kind: MaterialKind) -> Option<String> {
        let project = self.project.as_ref()?;
        let name = project.unique_file_name(MATERIALS_DIR, &format!("New {}", kind.label()), MATERIAL_EXT);
        let path = join(MATERIALS_DIR, &name);
        let def = MaterialDef::of_kind(stem_of(&name), kind);
        let written = project.abs(&path).map_err(|e| format!("{e:#}")).and_then(|p| {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            write_atomic(&p, def.to_json()).map_err(|e| format!("{e:#}"))
        });
        match written {
            Ok(()) => {
                self.own_writes.insert(path.clone());
                self.sync_assets(true);
                self.render_assets_changed();
                self.info(format!("Created {path}"));
                self.material_editor.open = true;
                self.select_material(&path);
                self.material_editor.renaming = Some(stem_of(&name).to_owned());
                Some(path)
            }
            Err(e) => {
                self.error(format!("could not create a material: {e}"));
                None
            }
        }
    }

    /// Copies the material being edited (`Name Copy.mat`) and edits the copy.
    pub fn duplicate_material(&mut self) {
        let Some(path) = self.material_editor.path.clone() else { return };
        self.flush_material(false, true);
        let Some(project) = self.project.as_ref() else { return };
        match project.duplicate_path(&path) {
            Ok(copy) => {
                self.sync_assets(true);
                self.render_assets_changed();
                self.select_material(&copy);
                self.info(format!("Duplicated {path} as {copy}"));
            }
            Err(e) => self.warn(format!("could not duplicate {path}: {e:#}")),
        }
    }

    /// Renames the material file (parts using it by path follow, like any renamed file).
    pub fn rename_material(&mut self, new_stem: &str) {
        let Some(path) = self.material_editor.path.clone() else { return };
        let stem = forge_engine::project::sanitize_file_stem(new_stem, "Material");
        if stem == stem_of(file_name(&path)) {
            return;
        }
        let new_name = format!("{stem}{MATERIAL_EXT}");
        self.flush_material(false, true);
        self.asset_rename(&path, &new_name);
        let new_path = join(parent_of(&path), &new_name);
        if self.asset_db().is_some_and(|db| db.entry(&new_path).is_some()) {
            self.render_assets_changed();
            self.select_material(&new_path);
        }
    }

    /// Asks before deleting the material being edited (parts that use it fall back to the engine's look).
    pub fn delete_material(&mut self) {
        if let Some(path) = self.material_editor.path.clone() {
            self.asset_ask_delete(vec![path]);
        }
    }

    /// Makes the selected parts draw with the material being edited.
    pub fn apply_material_to_selection(&mut self) {
        let Some(path) = self.material_editor.path.clone() else { return };
        let targets: Vec<_> = {
            let dm = self.dm.borrow();
            self.selection_ids().into_iter().filter(|id| dm.is_a(*id, "BasePart")).collect()
        };
        if targets.is_empty() {
            return self.warn("Select the parts to use this material on first.");
        }
        self.assign_asset_prop(&targets, "MaterialVariant", &path);
    }

    /// A texture slot of the material being edited takes a picture (empty: none).
    pub fn set_material_map(&mut self, slot: &str, path: &str) {
        let maps = &mut self.material_editor.def.maps;
        let field = match slot {
            "base_color" => &mut maps.base_color,
            "normal" => &mut maps.normal,
            "roughness" => &mut maps.roughness,
            "metallic" => &mut maps.metallic,
            "metallic_roughness" => &mut maps.metallic_roughness,
            "occlusion" => &mut maps.occlusion,
            "emission" => &mut maps.emission,
            _ => return,
        };
        *field = path.to_owned();
        self.material_changed();
        self.flush_material(false, true);
    }

    /// The `Material` enum items and the file each one stands for (for the window's "Used for" line): `(item, path)`.
    pub fn enum_items_using(&self, path: &str) -> Vec<String> {
        let stem = stem_of(file_name(path)).to_owned();
        match registry_material_items().into_iter().find(|i| i.eq_ignore_ascii_case(&stem)) {
            Some(item) => vec![item.to_owned()],
            None => Vec::new(),
        }
    }

    /// How many parts of the open scene use this material file (by `MaterialVariant`, or by the `Material` item named like it).
    pub fn parts_using_material(&self, path: &str) -> usize {
        let dm = self.dm.borrow();
        let stem = stem_of(file_name(path));
        dm.all_ids()
            .into_iter()
            .filter(|id| {
                dm.has_component(*id, "Appearance") && {
                    let variant = dm.str_ref(*id, "MaterialVariant");
                    variant == path || (!variant.is_empty() && variant.eq_ignore_ascii_case(stem)) || (variant.is_empty() && dm.str_ref(*id, "Material").eq_ignore_ascii_case(stem))
                }
            })
            .count()
    }
}

fn registry_material_items() -> Vec<&'static str> {
    forge_engine::core::enum_names("Material").to_vec()
}
