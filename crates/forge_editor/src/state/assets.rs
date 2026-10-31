//! The Asset Browser's state, and everything the editor does with the project's files.
//!
//! One system serves the Asset Browser, the script editor, the Scene Tree Explorer and the viewport:
//!
//! * [`Editor::sync_assets`] scans the real folders (`forge_engine::project::AssetDb`) about once a
//!   second and when asked. It notices files added, removed, changed or moved from outside the editor,
//!   keeps the lists the property pickers use, and reloads or flags the script being edited.
//! * Every file operation (create, rename, move, duplicate, delete, import) goes through the project's
//!   safe operations and then through [`Editor::apply_moves`], which points the scene in memory, the
//!   scenes on disk, the script editor and the browser at the new paths. A moved file keeps working.
//! * [`Editor::plan_asset`] decides what dropping or double-clicking an asset does; the Explorer, the
//!   viewport and the browser all ask it, so they agree and show the same feedback.
//!
//! Where a script file is stored never decides where it runs: a script object keeps the *path* of its
//! source file, and its class (`Script`, `LocalScript`, `ModuleScript`) decides the rest.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use bevy::math::{Vec2, Vec3};
use bevy_egui::egui;
use forge_engine::{
    assets::GameSource,
    core::{class, PropType, Value},
    dm::{DataModel, Id},
    project::{self, file_name, join, parent_of, stem_of, AssetDb, AssetKind},
};

use super::{Dialog, Editor, OpenScript};
use crate::dock::Panel;

/// How often the project's folders are looked at for changes made outside the editor.
pub const SCAN_INTERVAL: Duration = Duration::from_millis(1000);

/// Dragging assets (project-relative paths) between the browser, the Explorer and the viewport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetDrag {
    pub paths: Vec<String>,
}

/// Assets that were dropped on the viewport; the viewport finds what is under the pointer and finishes the drop.
#[derive(Clone, Debug)]
pub struct PendingDrop {
    pub paths: Vec<String>,
    pub pointer: Vec2,
}

/// A thumbnail of an image file, kept until the file changes.
pub struct Thumb {
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub texture: Option<egui::TextureHandle>,
}

pub struct AssetBrowser {
    /// The last scan (`None` before the first, or without a project).
    pub db: Option<AssetDb>,
    /// The folder being browsed (`""` is the project folder).
    pub dir: String,
    pub selected: Vec<String>,
    /// Where a Shift-click range starts.
    pub anchor: Option<String>,
    pub search: String,
    /// Folders that are open in the folder tree.
    pub open_folders: HashSet<String>,
    /// The item being renamed, with the text typed so far.
    pub renaming: Option<(String, String)>,
    pub last_scan: Option<Instant>,
    pub thumbs: HashMap<String, Thumb>,
    /// An image shown large in a window.
    pub preview: Option<String>,
    /// Width of a tile, in points.
    pub tile: f32,
}

impl Default for AssetBrowser {
    fn default() -> Self {
        AssetBrowser {
            db: None,
            dir: String::new(),
            selected: Vec::new(),
            anchor: None,
            search: String::new(),
            open_folders: HashSet::new(),
            renaming: None,
            last_scan: None,
            thumbs: HashMap::new(),
            preview: None,
            tile: 84.0,
        }
    }
}

/// What dropping or double-clicking an asset does.
#[derive(Clone, Debug, PartialEq)]
pub enum AssetPlan {
    OpenScene(String),
    OpenScript(String),
    Preview(String),
    /// Place the objects of a prefab inside `parent`.
    Instantiate { path: String, parent: Id },
    /// A new `MeshPart` showing the mesh, inside `parent`.
    CreateMesh { path: String, parent: Id, at: Option<Vec3> },
    /// Show this mesh on an existing `MeshPart`.
    AssignMesh { path: String, target: Id },
    /// A new script object of `class` using the file, inside `parent`.
    CreateScript { path: String, class: &'static str, parent: Id },
    /// Make an existing script object use the file.
    AssignScript { path: String, target: Id },
    /// Lay a picture over a part (its ColorMap) or show it on a decal / texture.
    AssignTexture { path: String, target: Id },
    /// Draw a part with a material file (its MaterialVariant).
    AssignMaterial { path: String, target: Id },
    /// Make a Sound play the file.
    AssignSound { path: String, target: Id },
    /// A new Sound playing the file, inside parent (inside a part or an attachment it is a 3D sound there).
    CreateSound { path: String, parent: Id },
    /// Edit the material in the Material Editor.
    OpenMaterial(String),
    /// Play the sound once, to hear it.
    PreviewSound(String),
}

impl AssetPlan {
    /// A short phrase for the drop feedback ("Place prefab in Workspace").
    pub fn describe(&self, dm: &DataModel) -> String {
        let name = |id: &Id| dm.name(*id);
        match self {
            AssetPlan::OpenScene(_) => "Open scene".into(),
            AssetPlan::OpenScript(_) => "Open in the script editor".into(),
            AssetPlan::Preview(_) => "Preview".into(),
            AssetPlan::Instantiate { parent, .. } => format!("Place prefab in {}", name(parent)),
            AssetPlan::CreateMesh { parent, .. } => format!("Create a MeshPart in {}", name(parent)),
            AssetPlan::AssignMesh { target, .. } => format!("Use this mesh on {}", name(target)),
            AssetPlan::CreateScript { class, parent, .. } => format!("Add a {class} to {}", name(parent)),
            AssetPlan::AssignScript { target, .. } => format!("Use this script on {}", name(target)),
            AssetPlan::AssignTexture { target, .. } => format!("Use this picture on {}", name(target)),
            AssetPlan::AssignMaterial { target, .. } => format!("Use this material on {}", name(target)),
            AssetPlan::AssignSound { target, .. } => format!("Play this sound from {}", name(target)),
            AssetPlan::CreateSound { parent, .. } => format!("Create a Sound in {}", name(parent)),
            AssetPlan::OpenMaterial(_) => "Open in the Material Editor".into(),
            AssetPlan::PreviewSound(_) => "Listen".into(),
        }
    }
}

fn file_stamp(path: &std::path::Path) -> Option<(u64, Option<SystemTime>)> {
    let meta = std::fs::metadata(path).ok()?;
    meta.is_file().then(|| (meta.len(), meta.modified().ok()))
}

impl Editor {
    pub fn asset_db(&self) -> Option<&AssetDb> {
        self.assets_ui.db.as_ref()
    }

    fn can_change_files(&mut self) -> bool {
        if self.project.is_none() {
            self.warn("Open a project first.");
            return false;
        }
        if self.play.is_some() {
            self.warn("Stop the game before changing project files.");
            return false;
        }
        true
    }

    // ---- keeping up with the disk --------------------------------------------------------------

    /// Looks at the project's folders (at most once a [`SCAN_INTERVAL`] unless `force`) and brings the
    /// editor up to date with what changed there, whoever changed it.
    pub fn sync_assets(&mut self, force: bool) {
        let Some(project) = self.project.as_ref() else {
            self.assets_ui.db = None;
            return;
        };
        if !force && self.assets_ui.last_scan.is_some_and(|t| t.elapsed() < SCAN_INTERVAL) {
            return;
        }
        self.assets_ui.last_scan = Some(Instant::now());
        let first = self.assets_ui.db.is_none();
        let (db, report) = AssetDb::scan(project, self.assets_ui.db.as_ref());
        self.assets_ui.db = Some(db);
        if !report.moved.is_empty() {
            self.info(format!("{} file(s) were moved outside the editor.", report.moved.len()));
            self.apply_moves(&report.moved);
        }
        self.note_changed_files(&report);
        if report.changed || first {
            self.update_file_lists();
            self.check_open_script();
            self.fix_browser_state();
        }
    }

    /// Files that the picture in the viewport or the sounds depend on changed (by someone else, or by a tool of the editor): what
    /// was read from them is forgotten, and the parts are drawn again with the new look.
    fn note_changed_files(&mut self, report: &project::SyncReport) {
        let relevant = |p: &String| matches!(AssetKind::of(file_name(p)), AssetKind::Mesh | AssetKind::Texture | AssetKind::Material | AssetKind::Audio);
        let own: Vec<String> = report.modified.iter().filter(|p| self.own_writes.remove(*p)).cloned().collect();
        let changed = report.modified.iter().chain(&report.added).chain(&report.removed).chain(report.moved.iter().map(|(_, n)| n)).filter(|p| relevant(p) && !own.contains(p)).count();
        if changed > 0 {
            self.render_assets_changed();
        }
        // Thumbnails of changed pictures are read again.
        for p in &report.modified {
            self.assets_ui.thumbs.remove(p);
            self.assets_ui.thumbs.remove(&format!("\u{1}preview:{p}"));
        }
    }

    /// Everything the renderer and the audio read from files is read again (a material, picture, model or sound changed).
    pub fn render_assets_changed(&mut self) {
        self.reload_meshes.set(true);
        self.material_editor.preview_textures.clear();
        if let Some(info) = self.dm.borrow().asset_info() {
            info.invalidate();
        }
    }

    /// The data model's view of the project's files (sizes of models, lengths of sounds) follows the open project.
    fn refresh_asset_info(&mut self) {
        let Some(project) = self.project.as_ref() else {
            self.dm.borrow_mut().set_asset_info(None);
            return;
        };
        let scale = project.manifest.assets.mesh_scale();
        let current = self.dm.borrow().asset_info();
        match current {
            Some(info) if matches!(info.source(), GameSource::Dir(d) if *d == project.root) => info.set_unit_scale(scale),
            _ => {
                let info = std::rc::Rc::new(forge_engine::assets::info::AssetInfo::new(GameSource::Dir(project.root.clone())));
                info.set_unit_scale(scale);
                self.dm.borrow_mut().set_asset_info(Some(info));
            }
        }
    }

    /// The lists the property pickers and menus use, from the last scan.
    fn update_file_lists(&mut self) {
        let Some(db) = self.assets_ui.db.as_ref() else { return };
        let of = |wanted: &dyn Fn(AssetKind) -> bool| -> Vec<String> { db.entries().iter().filter(|e| wanted(e.kind)).map(|e| e.path.clone()).collect() };
        let (scenes, scripts) = (of(&|k| k == AssetKind::Scene), of(&|k| k == AssetKind::Script));
        let assets: Vec<String> = db.entries().iter().filter(|e| !AssetDb::is_manifest(&e.path) && !matches!(e.kind, AssetKind::Scene | AssetKind::Script | AssetKind::Prefab)).map(|e| e.path.clone()).collect();
        self.scenes = scenes;
        self.scripts = scripts;
        self.assets = assets;
        if let Some(p) = &self.project {
            self.source = GameSource::Dir(p.root.clone());
        }
        self.refresh_asset_info();
    }

    /// The browsed folder must exist, and the selection and rename must point at things that do.
    fn fix_browser_state(&mut self) {
        let Some(db) = self.assets_ui.db.as_ref() else { return };
        let mut dir = self.assets_ui.dir.clone();
        while !db.has_folder(&dir) {
            dir = parent_of(&dir).to_owned();
        }
        let exists = |p: &String| db.entry(p).is_some() || db.has_folder(p);
        let selected: Vec<String> = self.assets_ui.selected.iter().filter(|p| exists(p)).cloned().collect();
        let anchor = self.assets_ui.anchor.take().filter(|p| exists(p));
        let renaming = self.assets_ui.renaming.take().filter(|(p, _)| exists(p));
        self.assets_ui.dir = dir;
        self.assets_ui.selected = selected;
        self.assets_ui.anchor = anchor;
        self.assets_ui.renaming = renaming;
        self.assets_ui.preview = self.assets_ui.preview.take().filter(|p| self.assets_ui.db.as_ref().is_some_and(|db| db.entry(p).is_some()));
    }

    /// The script being edited was changed or removed on disk: reload it, or flag the clash.
    fn check_open_script(&mut self) {
        let (Some(db), Some(project)) = (self.assets_ui.db.as_ref(), self.project.as_ref()) else { return };
        let Some(s) = self.script.as_mut() else { return };
        let Some(entry) = db.entry(&s.path) else {
            s.missing = true;
            return;
        };
        s.missing = false;
        let on_disk = Some((entry.size, entry.modified));
        if on_disk == s.seen {
            return;
        }
        s.seen = on_disk;
        if on_disk == s.stamp {
            return; // that is the file as this editor wrote or read it
        }
        if s.modified {
            s.conflict = true;
        } else if let Ok(text) = project.abs(&s.path).and_then(|p| Ok(std::fs::read_to_string(p)?)) {
            s.text = text;
            s.stamp = on_disk;
            let path = s.path.clone();
            self.info(format!("{path} changed on disk; reloaded it."));
        }
    }

    /// Files are now at new places (`old -> new`, one pair per file): points everything that uses
    /// them at the new paths. Used for moves made here and, from a scan, for moves made outside.
    pub fn apply_moves(&mut self, moved: &[(String, String)]) {
        if moved.is_empty() {
            return;
        }
        let map: HashMap<String, String> = moved.iter().cloned().collect();
        if let Some(db) = self.assets_ui.db.as_mut() {
            db.record_moves(moved);
        }
        if let Some(open) = self.scene_path.as_ref().and_then(|p| map.get(p)) {
            self.scene_path = Some(open.clone());
        }
        if let Some(s) = self.script.as_mut() {
            if let Some(new) = map.get(&s.path) {
                s.path = new.clone();
            }
        }
        for list in [&mut self.assets_ui.selected] {
            for p in list.iter_mut() {
                if let Some(new) = map.get(p) {
                    *p = new.clone();
                }
            }
        }
        if let Some(a) = self.assets_ui.anchor.as_mut() {
            if let Some(new) = map.get(a) {
                *a = new.clone();
            }
        }
        for (old, new) in moved {
            if let Some(thumb) = self.assets_ui.thumbs.remove(old) {
                self.assets_ui.thumbs.insert(new.clone(), thumb);
            }
        }

        // The scenes on disk (the open one is updated in memory below).
        let skip = self.scene_path.clone();
        let mut changed = 0;
        if let Some(project) = self.project.as_mut() {
            match project.remap_references(moved, skip.as_deref()) {
                Ok(report) => changed += report.references,
                Err(e) => self.error(format!("could not update references to moved files: {e:#}")),
            }
        }
        // The scene in memory. While a game runs, the data model is the running game: the scene
        // is not touched (and what Stop restores is updated with it).
        if self.play.is_none() {
            let n = self.remap_scene_in_memory(&map);
            if n > 0 {
                changed += n;
                self.dirty = true;
            }
        }
        if changed > 0 {
            self.info(format!("Updated {changed} reference(s) to moved files."));
        }
        if self.dirty && self.play.is_none() {
            self.save_all();
        }
    }

    /// Points every script / asset path property of the open scene that is a key of `map` at its value.
    fn remap_scene_in_memory(&mut self, map: &HashMap<String, String>) -> usize {
        let mut dm = self.dm.borrow_mut();
        let mut changed = 0;
        for id in dm.all_ids() {
            let Some(c) = dm.get(id).map(|i| i.class) else { continue };
            for rp in c.props.iter().filter(|p| p.meta.ty == PropType::ScriptPath || matches!(p.meta.ty, PropType::Asset(_))) {
                if let Some(Value::Str(s)) = dm.get_prop(id, rp.meta.name) {
                    if let Some(new) = map.get(&s) {
                        if dm.set_prop(id, rp.meta.name, Value::Str(new.clone())).is_ok() {
                            changed += 1;
                        }
                    }
                }
            }
        }
        changed
    }

    // ---- navigating -----------------------------------------------------------------------------

    /// Shows a folder in the browser (opening it in the folder tree).
    pub fn browse_to(&mut self, dir: &str) {
        let mut cur = dir.to_owned();
        loop {
            cur = parent_of(&cur).to_owned();
            if cur.is_empty() {
                break;
            }
            self.assets_ui.open_folders.insert(cur.clone());
        }
        if !dir.is_empty() {
            self.assets_ui.open_folders.insert(dir.to_owned());
        }
        self.assets_ui.dir = dir.to_owned();
        self.assets_ui.selected.clear();
        self.assets_ui.renaming = None;
    }

    /// Shows the folder an asset is in, with the asset selected (the search is cleared so it is listed).
    pub fn reveal_asset(&mut self, path: &str) {
        self.assets_ui.search.clear();
        self.browse_to(parent_of(path));
        self.assets_ui.selected = vec![path.to_owned()];
        self.assets_ui.anchor = Some(path.to_owned());
        self.show_panel(Panel::Assets);
    }

    /// A click on a tile: plain selects only it, `Ctrl` adds / removes it, `Shift` selects the range from
    /// the anchor. `order` is what the grid lists, top to bottom.
    pub fn click_asset(&mut self, order: &[String], path: &str, ctrl: bool, shift: bool) {
        let ui = &mut self.assets_ui;
        match (ctrl, shift) {
            (false, false) => {
                ui.selected = vec![path.to_owned()];
                ui.anchor = Some(path.to_owned());
            }
            (true, false) => {
                if let Some(at) = ui.selected.iter().position(|p| p == path) {
                    ui.selected.remove(at);
                } else {
                    ui.selected.push(path.to_owned());
                }
                ui.anchor = Some(path.to_owned());
            }
            (_, true) => {
                let at = |p: &str| order.iter().position(|o| o == p);
                match (ui.anchor.as_deref().and_then(at), at(path)) {
                    (Some(a), Some(b)) => {
                        let (lo, hi) = (a.min(b), a.max(b));
                        let mut chosen: Vec<String> = if ctrl { ui.selected.clone() } else { Vec::new() };
                        for p in &order[lo..=hi] {
                            if !chosen.contains(p) {
                                chosen.push(p.clone());
                            }
                        }
                        ui.selected = chosen;
                    }
                    _ => {
                        ui.selected = vec![path.to_owned()];
                        ui.anchor = Some(path.to_owned());
                    }
                }
            }
        }
    }

    // ---- file operations ------------------------------------------------------------------------

    /// Records and applies the result of an operation that moved files, then looks at the disk again.
    fn finish_file_op(&mut self, moved: &[(String, String)]) {
        self.apply_moves(moved);
        self.sync_assets(true);
        // (A move changes no file but its path, which the scan may not count as a change.)
        self.update_file_lists();
        self.fix_browser_state();
    }

    pub fn asset_new_folder(&mut self) {
        if !self.can_change_files() {
            return;
        }
        let dir = self.assets_ui.dir.clone();
        let Some(project) = &self.project else { return };
        let mut n = 1;
        let name = loop {
            let c = if n == 1 { "New Folder".to_owned() } else { format!("New Folder {n}") };
            if !project.root.join(join(&dir, &c)).exists() {
                break c;
            }
            n += 1;
        };
        match project.create_folder(&dir, &name) {
            Ok(rel) => {
                self.sync_assets(true);
                self.start_asset_rename(&rel);
            }
            Err(e) => self.warn(format!("{e:#}")),
        }
    }

    /// A new empty Luau file in the browsed folder (not attached to any object), ready to be named.
    pub fn asset_new_script(&mut self, class: &str) {
        if !self.can_change_files() {
            return;
        }
        let dir = self.assets_ui.dir.clone();
        let Some(project) = &self.project else { return };
        let base = match class {
            "LocalScript" => "NewLocalScript",
            "ModuleScript" => "NewModule",
            _ => "NewScript",
        };
        match project.create_script(Some(&dir), base, class) {
            Ok(rel) => {
                self.sync_assets(true);
                self.start_asset_rename(&rel);
            }
            Err(e) => self.warn(format!("{e:#}")),
        }
    }

    pub fn start_asset_rename(&mut self, path: &str) {
        let name = file_name(path).to_owned();
        self.assets_ui.selected = vec![path.to_owned()];
        self.assets_ui.anchor = Some(path.to_owned());
        self.assets_ui.renaming = Some((path.to_owned(), name));
    }

    /// Finishes (or abandons) the rename started with [`Editor::start_asset_rename`].
    pub fn finish_asset_rename(&mut self, accept: bool) {
        let Some((path, new_name)) = self.assets_ui.renaming.take() else { return };
        let new_name = new_name.trim().to_owned();
        if accept && !new_name.is_empty() && new_name != file_name(&path) {
            self.asset_rename(&path, &new_name);
        }
    }

    pub fn asset_rename(&mut self, path: &str, new_name: &str) {
        if !self.can_change_files() {
            return;
        }
        let Some(project) = &self.project else { return };
        match project.rename_path(path, new_name) {
            Ok(moved) => {
                let new_path = join(parent_of(path), new_name);
                self.info(format!("Renamed {path} to {new_name}"));
                self.follow_folder(path, &new_path);
                self.finish_file_op(&moved);
                self.assets_ui.selected = vec![new_path.clone()];
                self.assets_ui.anchor = Some(new_path);
            }
            Err(e) => self.warn(format!("could not rename {path}: {e:#}")),
        }
    }

    /// The browsed folder, the open folders and the selection follow a folder that moved.
    fn follow_folder(&mut self, old: &str, new: &str) {
        let swap = |p: &str| -> Option<String> {
            if p == old {
                Some(new.to_owned())
            } else {
                p.strip_prefix(&format!("{old}/")).map(|rest| format!("{new}/{rest}"))
            }
        };
        if let Some(d) = swap(&self.assets_ui.dir) {
            self.assets_ui.dir = d;
        }
        let open: HashSet<String> = self.assets_ui.open_folders.iter().map(|p| swap(p).unwrap_or_else(|| p.clone())).collect();
        self.assets_ui.open_folders = open;
    }

    pub fn asset_move(&mut self, paths: &[String], dest: &str) {
        if !self.can_change_files() {
            return;
        }
        let mut all: Vec<(String, String)> = Vec::new();
        let mut moved_paths: Vec<String> = Vec::new();
        for path in paths {
            let Some(project) = &self.project else { return };
            match project.move_path(path, dest) {
                Ok(pairs) if pairs.is_empty() => {}
                Ok(pairs) => {
                    let new_path = join(dest, file_name(path));
                    self.follow_folder(path, &new_path);
                    moved_paths.push(new_path);
                    all.extend(pairs);
                }
                Err(e) => self.warn(format!("could not move {path}: {e:#}")),
            }
        }
        if !all.is_empty() {
            self.info(format!("Moved {} item(s) to {}", moved_paths.len(), if dest.is_empty() { "the project folder" } else { dest }));
        }
        self.finish_file_op(&all);
        self.assets_ui.selected = moved_paths;
    }

    pub fn asset_duplicate(&mut self, paths: &[String]) {
        if !self.can_change_files() {
            return;
        }
        let mut copies = Vec::new();
        for path in paths {
            let Some(project) = &self.project else { return };
            match project.duplicate_path(path) {
                Ok(copy) => copies.push(copy),
                Err(e) => self.warn(format!("could not duplicate {path}: {e:#}")),
            }
        }
        self.sync_assets(true);
        if !copies.is_empty() {
            self.info(format!("Duplicated {} item(s)", copies.len()));
            self.assets_ui.selected = copies;
        }
    }

    /// What deleting these would break: the scenes (and the manifest) that use them. The scene that is
    /// open counts as it is now, in memory, not as it was last saved.
    pub fn assets_used_by(&self, paths: &[String]) -> Vec<String> {
        let Some(db) = self.asset_db() else { return Vec::new() };
        let inside = |file: &str| paths.iter().any(|p| file == p || file.starts_with(&format!("{p}/")));
        let mut users: Vec<String> = paths.iter().flat_map(|p| db.used_by(p)).filter(|u| !inside(u)).collect();
        if let Some(open) = self.scene_path.as_ref() {
            users.retain(|u| u != open);
            let dm = self.dm.borrow();
            let live = dm.all_ids().into_iter().any(|id| {
                dm.get(id).is_some_and(|i| i.class.props.iter().any(|p| (p.meta.ty == PropType::ScriptPath || matches!(p.meta.ty, PropType::Asset(_))) && matches!(dm.get_prop(id, p.meta.name), Some(Value::Str(s)) if !s.is_empty() && inside(&s))))
            });
            if live && !inside(open) {
                users.push(open.clone());
            }
        }
        users.sort();
        users.dedup();
        users
    }

    /// Asks before deleting (see `Dialog::DeleteAssets`).
    pub fn asset_ask_delete(&mut self, paths: Vec<String>) {
        if paths.is_empty() || !self.can_change_files() {
            return;
        }
        let used_by = self.assets_used_by(&paths);
        self.dialog = Dialog::DeleteAssets { paths, used_by };
    }

    /// Deletes (moves into `.forge/trash`) files and folders. The open scene cannot be deleted.
    pub fn asset_delete(&mut self, paths: &[String]) {
        if !self.can_change_files() {
            return;
        }
        let mut deleted = 0;
        for path in paths {
            let holds_open_scene = self.scene_path.as_deref().is_some_and(|s| s == path || s.starts_with(&format!("{path}/")));
            if holds_open_scene {
                self.warn(format!("{path} contains the scene that is open; open another scene first."));
                continue;
            }
            let Some(project) = &self.project else { return };
            match project.trash_path(path) {
                Ok(to) => {
                    deleted += 1;
                    self.info(format!("Deleted {path} (it can be recovered from {to})"));
                    if self.script.as_ref().is_some_and(|s| s.path == *path || s.path.starts_with(&format!("{path}/"))) {
                        self.script = None;
                    }
                }
                Err(e) => self.warn(format!("could not delete {path}: {e:#}")),
            }
        }
        if deleted > 0 {
            self.assets_ui.selected.clear();
            self.sync_assets(true);
        }
    }

    /// Copies files or folders from anywhere on disk into the browsed folder.
    pub fn asset_import(&mut self, sources: Vec<PathBuf>) {
        if sources.is_empty() || !self.can_change_files() {
            return;
        }
        let dir = self.assets_ui.dir.clone();
        let Some(project) = &self.project else { return };
        let results = project.import_files(&sources, &dir);
        let mut imported = Vec::new();
        for (source, outcome) in results {
            match outcome {
                Ok(rel) => imported.push(rel),
                Err(e) => self.warn(format!("could not import {}: {e}", source.display())),
            }
        }
        self.sync_assets(true);
        if !imported.is_empty() {
            self.info(format!("Imported {} item(s) into {}", imported.len(), if dir.is_empty() { "the project folder" } else { &dir }));
            self.assets_ui.selected = imported;
        }
    }

    /// Reads a file's details again and drops its thumbnail; objects using a changed mesh show the new one.
    pub fn asset_reimport(&mut self, path: &str) {
        self.assets_ui.thumbs.remove(path);
        self.sync_assets(true);
        if self.asset_db().and_then(|db| db.entry(path)).is_none() {
            return self.warn(format!("{path} no longer exists."));
        }
        if matches!(AssetKind::of(file_name(path)), AssetKind::Mesh | AssetKind::Texture | AssetKind::Material | AssetKind::Audio) {
            self.render_assets_changed();
        }
        self.info(format!("Reimported {path}"));
    }

    // ---- scripts and their files ----------------------------------------------------------------

    /// Remembers that a script object was renamed; its file is renamed to match once typing stops
    /// (the Properties panel renames on every keystroke, and a file must not follow each letter).
    pub fn queue_script_rename(&mut self, id: Id, old_name: &str) {
        match self.script_renames.iter_mut().find(|r| r.0 == id) {
            Some(r) => r.2 = Instant::now(),
            None => self.script_renames.push((id, old_name.to_owned(), Instant::now())),
        }
    }

    /// Renames the files of scripts whose objects were renamed a moment ago (see [Editor::queue_script_rename]).
    pub fn flush_script_renames(&mut self, typing: bool) {
        if self.script_renames.is_empty() || typing {
            return;
        }
        let due: Vec<(Id, String, Instant)> = self.script_renames.iter().filter(|r| r.2.elapsed() >= Duration::from_millis(400)).cloned().collect();
        self.script_renames.retain(|r| r.2.elapsed() < Duration::from_millis(400));
        for (id, original, _) in due {
            if self.dm.borrow().exists(id) {
                self.script_renamed(id, &original);
            }
        }
    }

    /// An object called `old_name` was renamed. If it is a script whose file is named after it (and
    /// no other object uses that file), the file is renamed to match; the object's reference follows.
    pub fn script_renamed(&mut self, id: Id, old_name: &str) {
        if self.play.is_some() {
            return;
        }
        let (path, new_name) = {
            let dm = self.dm.borrow();
            let Some(inst) = dm.get(id) else { return };
            if inst.class.prop("SourceFile").is_none_or(|p| p.meta.ty != PropType::ScriptPath) {
                return;
            }
            let path = dm.string(id, "SourceFile");
            if path.is_empty() {
                return;
            }
            // Another object using the same file: its name would be left behind, so leave the file alone.
            let shared = dm.all_ids().into_iter().any(|o| o != id && dm.get(o).is_some_and(|i| i.class.prop("SourceFile").is_some()) && dm.string(o, "SourceFile") == path);
            if shared {
                return;
            }
            (path, inst.name.clone())
        };
        let stem = stem_of(file_name(&path)).to_owned();
        let old_stem = project::sanitize_file_stem(old_name, "");
        let was_named_after_it = !old_stem.is_empty() && stem.strip_prefix(&old_stem).is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()));
        if !was_named_after_it {
            return;
        }
        let ext = &file_name(&path)[stem.len()..];
        let wanted = format!("{}{ext}", project::sanitize_file_stem(&new_name, "Script"));
        if wanted == file_name(&path) {
            return;
        }
        let Some(project) = &self.project else { return };
        if !project.abs(&path).is_ok_and(|p| p.is_file()) {
            return;
        }
        match project.rename_path(&path, &wanted) {
            Ok(moved) => {
                self.info(format!("Renamed {path} to {wanted} to match the script's name"));
                self.finish_file_op(&moved);
            }
            Err(e) => self.info(format!("kept the file name {} for the script ({e:#})", file_name(&path))),
        }
    }

    /// The script files that objects under `roots` use (before they are deleted).
    pub(super) fn script_files_under(&self, roots: &[Id]) -> Vec<String> {
        let dm = self.dm.borrow();
        let mut files = Vec::new();
        for root in roots {
            for id in dm.subtree(*root) {
                if dm.get(id).is_some_and(|i| i.class.prop("SourceFile").is_some()) {
                    let f = dm.string(id, "SourceFile");
                    if !f.is_empty() && !files.contains(&f) {
                        files.push(f);
                    }
                }
            }
        }
        files
    }

    /// After script objects were deleted: their files stay in the project (they may be used elsewhere or
    /// be worth keeping); says so.
    pub(super) fn note_kept_script_files(&mut self, files: &[String]) {
        let Some(project) = &self.project else { return };
        let existing: Vec<&String> = files.iter().filter(|f| project.abs(f).is_ok_and(|p| p.is_file())).collect();
        if existing.is_empty() {
            return;
        }
        let list = existing.iter().take(3).map(|f| f.as_str()).collect::<Vec<_>>().join(", ");
        let more = existing.len().saturating_sub(3);
        self.info(format!("Kept the script file(s) {list}{} in the project; delete them from the Assets tab if they are not needed.", if more > 0 { format!(" and {more} more") } else { String::new() }));
    }

    // ---- using assets ---------------------------------------------------------------------------

    /// The container an object created "at" `target` goes into: the target itself when it holds
    /// other objects, else the one it sits in, else the Workspace.
    fn container_for(&self, target: Option<Id>) -> Id {
        let dm = self.dm.borrow();
        let mut cur = target.filter(|t| dm.exists(*t));
        while let Some(t) = cur {
            if dm.get(t).is_some_and(|i| i.class.desc.container && !i.class.desc.root) {
                return t;
            }
            cur = dm.parent(t);
        }
        dm.workspace
    }

    /// Where a sound dropped at `target` goes when it is not inside an object: SoundService, unless the drop was inside another container.
    fn sound_container_for(&self, target: Option<Id>) -> Id {
        let dm = self.dm.borrow();
        match target.filter(|t| dm.exists(*t)).map(|t| self.container_for(Some(t))) {
            Some(c) if c != dm.workspace => c,
            _ => dm.service("SoundService").unwrap_or(dm.workspace),
        }
    }

    /// The class a script file becomes when it is added to the scene at `target`: the class of an object
    /// that already uses the file, else one that suits the service it goes into.
    fn script_class_for(&self, path: &str, target: Id) -> &'static str {
        let dm = self.dm.borrow();
        let user = dm.all_ids().into_iter().find(|id| dm.get(*id).is_some_and(|i| i.class.prop("SourceFile").is_some() && i.class.is_a("LuaSourceContainer")) && dm.string(*id, "SourceFile") == path);
        if let Some(c) = user.and_then(|u| dm.get(u)).map(|i| i.class.name) {
            return match c {
                "LocalScript" => "LocalScript",
                "ModuleScript" => "ModuleScript",
                _ => "Script",
            };
        }
        let mut cur = Some(target);
        while let Some(t) = cur {
            match dm.get(t).map(|i| i.class.name) {
                Some("StarterPlayer" | "StarterGui" | "ReplicatedFirst") => return "LocalScript",
                Some("ReplicatedStorage" | "ServerStorage") => return "ModuleScript",
                _ => {}
            }
            cur = dm.parent(t);
        }
        "Script"
    }

    /// The single selected object, if it is one.
    fn only_selected(&self) -> Option<Id> {
        let dm = self.dm.borrow();
        let ids = self.selection_ids();
        (ids.len() == 1).then(|| ids[0]).filter(|id| dm.exists(*id))
    }

    /// What `path` would do on `target`. `dropped`: it was dragged there (else double-clicked, and
    /// `target` is ignored in favour of the selection). `Err` says why it cannot be done.
    pub fn plan_asset(&self, path: &str, target: Option<Id>, at: Option<Vec3>, dropped: bool) -> Result<AssetPlan, String> {
        let name = file_name(path);
        let kind = AssetKind::of(name);
        let target = if dropped { target } else { self.only_selected() };
        let is_a = |id: Id, c: &str| self.dm.borrow().get(id).is_some_and(|i| i.class.is_a(c));
        match kind {
            AssetKind::Scene if dropped => Err(format!("{name} is a scene: double-click it to open it. To place objects from one scene in another, save them as a prefab.")),
            AssetKind::Scene => Ok(AssetPlan::OpenScene(path.to_owned())),
            AssetKind::Prefab => Ok(AssetPlan::Instantiate { path: path.to_owned(), parent: self.container_for(target) }),
            AssetKind::Script => {
                if !dropped {
                    return Ok(AssetPlan::OpenScript(path.to_owned()));
                }
                match target {
                    Some(t) if is_a(t, "LuaSourceContainer") => {
                        if self.dm.borrow().string(t, "SourceFile") == path {
                            Err(format!("{} already uses {name}", self.dm.borrow().name(t)))
                        } else {
                            Ok(AssetPlan::AssignScript { path: path.to_owned(), target: t })
                        }
                    }
                    _ => {
                        let parent = match target {
                            Some(t) => t,
                            None => {
                                let dm = self.dm.borrow();
                                dm.service("ServerScriptService").unwrap_or(dm.workspace)
                            }
                        };
                        let dm = self.dm.borrow();
                        if dm.children(parent).iter().any(|c| dm.get(*c).is_some_and(|i| i.class.prop("SourceFile").is_some()) && dm.string(*c, "SourceFile") == path) {
                            return Err(format!("{} already has a script using {name}", dm.name(parent)));
                        }
                        drop(dm);
                        Ok(AssetPlan::CreateScript { path: path.to_owned(), class: self.script_class_for(path, parent), parent })
                    }
                }
            }
            AssetKind::Mesh => {
                if !AssetKind::usable_by_engine(name) {
                    return Err(AssetKind::unusable_reason(name));
                }
                match target {
                    Some(t) if is_a(t, "MeshPart") => {
                        if self.dm.borrow().string(t, "MeshId") == path {
                            Err(format!("{} already shows {name}", self.dm.borrow().name(t)))
                        } else {
                            Ok(AssetPlan::AssignMesh { path: path.to_owned(), target: t })
                        }
                    }
                    _ => Ok(AssetPlan::CreateMesh { path: path.to_owned(), parent: self.container_for(target), at }),
                }
            }
            AssetKind::Texture if !dropped => Ok(AssetPlan::Preview(path.to_owned())),
            AssetKind::Texture => {
                if !AssetKind::usable_by_engine(name) {
                    return Err(AssetKind::unusable_reason(name));
                }
                match target {
                    Some(t) if is_a(t, "Decal") => Ok(AssetPlan::AssignTexture { path: path.to_owned(), target: t }),
                    Some(t) if is_a(t, "BasePart") => Ok(AssetPlan::AssignTexture { path: path.to_owned(), target: t }),
                    _ => Err(format!("Drop {name} on a part to lay it over the part, or on a Decal; to use it in a material, drop it on a texture slot in the Material Editor.")),
                }
            }
            AssetKind::Material if !dropped => Ok(AssetPlan::OpenMaterial(path.to_owned())),
            AssetKind::Material => match target {
                Some(t) if is_a(t, "BasePart") => Ok(AssetPlan::AssignMaterial { path: path.to_owned(), target: t }),
                _ => Err(format!("Drop {name} on a part to draw it with this material (double-click it to edit it).")),
            },
            AssetKind::Audio if !dropped => Ok(AssetPlan::PreviewSound(path.to_owned())),
            AssetKind::Audio => {
                if !AssetKind::usable_by_engine(name) {
                    return Err(AssetKind::unusable_reason(name));
                }
                match target {
                    Some(t) if is_a(t, "Sound") => Ok(AssetPlan::AssignSound { path: path.to_owned(), target: t }),
                    // Inside a part (or attachment, or model) it is a sound that comes from there.
                    Some(t) if is_a(t, "BasePart") || is_a(t, "Attachment") || is_a(t, "Model") => Ok(AssetPlan::CreateSound { path: path.to_owned(), parent: t }),
                    _ => Ok(AssetPlan::CreateSound { path: path.to_owned(), parent: self.sound_container_for(target) }),
                }
            }
            AssetKind::Folder => Err("Folders are opened in the Assets tab.".to_owned()),
            other => Err(format!("{} files are listed here, but nothing in this version of Forge can use them yet.", other.label())),
        }
    }

    /// Does what [`Editor::plan_asset`] decided. Returns the objects that were created or changed.
    pub fn run_plan(&mut self, plan: AssetPlan) -> Vec<Id> {
        match plan {
            AssetPlan::OpenScene(path) => {
                self.load_scene(&path);
                Vec::new()
            }
            AssetPlan::OpenScript(path) => {
                self.open_script(&path);
                Vec::new()
            }
            AssetPlan::Preview(path) => {
                self.assets_ui.preview = Some(path);
                Vec::new()
            }
            AssetPlan::Instantiate { path, parent } => {
                let Some(project) = &self.project else { return Vec::new() };
                let place = match project.read_place(&path) {
                    Ok(p) => p,
                    Err(e) => {
                        self.error(format!("{e:#}"));
                        return Vec::new();
                    }
                };
                let result = self.dm.borrow_mut().instantiate_place(&place, parent);
                match result {
                    Ok((made, warns)) => {
                        for w in warns {
                            self.warn(format!("{path}: {w}"));
                        }
                        self.select_created(&made);
                        self.info(format!("Placed {} object(s) from {path}", made.len()));
                        made
                    }
                    Err(e) => {
                        self.warn(format!("could not place {path}: {e}"));
                        Vec::new()
                    }
                }
            }
            AssetPlan::CreateMesh { path, parent, at } => {
                let at = at.unwrap_or_else(|| self.focus_point());
                let made: Result<Id, String> = (|| {
                    let mut dm = self.dm.borrow_mut();
                    let id = dm.create("MeshPart")?;
                    dm.set_name(id, stem_of(file_name(&path)));
                    dm.set_prop(id, "MeshId", Value::Str(path.clone()))?;
                    if let Some(c) = class("MeshPart") {
                        for (prop, default) in c.desc.insert_props {
                            dm.set_prop(id, prop, default.to_value())?;
                        }
                    }
                    // The model appears at the size it was made (at the project's import scale), not squeezed into a default box.
                    if let Some(size) = dm.assets().and_then(|a| a.mesh_size(&path)) {
                        dm.set_prop(id, "Size", Value::V3(size))?;
                    }
                    dm.set_prop(id, "Position", Value::V3(at))?;
                    dm.set_parent(id, Some(parent))?;
                    Ok(id)
                })();
                match made {
                    Ok(id) => {
                        self.select_created(&[id]);
                        self.info(format!("Created a MeshPart for {path}"));
                        vec![id]
                    }
                    Err(e) => {
                        self.warn(e);
                        Vec::new()
                    }
                }
            }
            AssetPlan::AssignMesh { path, target } => {
                let r = {
                    let mut dm = self.dm.borrow_mut();
                    let first_mesh = dm.string(target, "MeshId").is_empty();
                    let r = dm.set_prop(target, "MeshId", Value::Str(path.clone()));
                    // The first model a part gets takes the model's own size; changing the model of a part that has one keeps the
                    // part's size (the new model is fitted to it, as a script setting MeshId would).
                    if r.is_ok() && first_mesh {
                        if let Some(size) = dm.assets().and_then(|a| a.mesh_size(&path)) {
                            let _ = dm.set_prop(target, "Size", Value::V3(size));
                        }
                    }
                    r
                };
                self.finish_assign(target, &path, r)
            }
            AssetPlan::CreateScript { path, class: script_class, parent } => {
                let made: Result<Id, String> = (|| {
                    let mut dm = self.dm.borrow_mut();
                    let id = dm.create(script_class)?;
                    dm.set_name(id, stem_of(file_name(&path)));
                    dm.set_prop(id, "SourceFile", Value::Str(path.clone()))?;
                    dm.set_parent(id, Some(parent))?;
                    Ok(id)
                })();
                match made {
                    Ok(id) => {
                        self.select_created(&[id]);
                        self.info(format!("Added a {script_class} using {path}"));
                        let runs = {
                            let dm = self.dm.borrow();
                            let mut cur = Some(parent);
                            let mut runs = false;
                            while let Some(c) = cur {
                                runs |= dm.get(c).is_some_and(|i| i.class.desc.runs_scripts);
                                cur = dm.parent(c);
                            }
                            runs
                        };
                        if script_class != "ModuleScript" && !runs {
                            self.warn("Scripts only run inside Workspace, ServerScriptService, StarterPlayer and ReplicatedFirst; this one is outside them.");
                        }
                        vec![id]
                    }
                    Err(e) => {
                        self.warn(e);
                        Vec::new()
                    }
                }
            }
            AssetPlan::AssignScript { path, target } => {
                let r = self.dm.borrow_mut().set_prop(target, "SourceFile", Value::Str(path.clone()));
                self.finish_assign(target, &path, r)
            }
            AssetPlan::AssignTexture { path, target } => {
                let r = {
                    let mut dm = self.dm.borrow_mut();
                    let prop = if dm.is_a(target, "Decal") { "Texture" } else { "ColorMap" };
                    dm.set_prop(target, prop, Value::Str(path.clone()))
                };
                self.finish_assign(target, &path, r)
            }
            AssetPlan::AssignMaterial { path, target } => {
                let r = self.dm.borrow_mut().set_prop(target, "MaterialVariant", Value::Str(path.clone()));
                self.finish_assign(target, &path, r)
            }
            AssetPlan::AssignSound { path, target } => {
                let r = self.dm.borrow_mut().set_prop(target, "SoundId", Value::Str(path.clone()));
                self.finish_assign(target, &path, r)
            }
            AssetPlan::CreateSound { path, parent } => {
                let made: Result<Id, String> = (|| {
                    let mut dm = self.dm.borrow_mut();
                    let id = dm.create("Sound")?;
                    dm.set_name(id, stem_of(file_name(&path)));
                    dm.set_prop(id, "SoundId", Value::Str(path.clone()))?;
                    dm.set_parent(id, Some(parent))?;
                    Ok(id)
                })();
                match made {
                    Ok(id) => {
                        self.select_created(&[id]);
                        self.info(format!("Created a Sound for {path}"));
                        vec![id]
                    }
                    Err(e) => {
                        self.warn(e);
                        Vec::new()
                    }
                }
            }
            AssetPlan::OpenMaterial(path) => {
                self.open_material_editor(Some(&path));
                Vec::new()
            }
            AssetPlan::PreviewSound(path) => {
                self.preview_sound(&path);
                Vec::new()
            }
        }
    }

    fn finish_assign(&mut self, target: Id, path: &str, result: Result<(), String>) -> Vec<Id> {
        match result {
            Ok(()) => {
                self.dirty = true;
                let name = self.dm.borrow().name(target);
                self.info(format!("{name} now uses {path}"));
                self.select_created(&[target]);
                vec![target]
            }
            Err(e) => {
                self.warn(e);
                Vec::new()
            }
        }
    }

    fn select_created(&mut self, ids: &[Id]) {
        if let Some(last) = ids.last().copied() {
            self.selection = ids.to_vec();
            self.selected = Some(last);
            self.last_inspected = Some(last);
            self.reveal = true;
            self.dirty = true;
        }
    }

    /// Double-clicking an asset in the browser.
    pub fn open_asset(&mut self, path: &str) {
        if self.asset_db().is_some_and(|db| db.has_folder(path)) {
            self.assets_ui.search.clear();
            return self.browse_to(path);
        }
        match self.plan_asset(path, None, None, false) {
            Ok(plan) => {
                // A mesh that is already on the selected MeshPart is not applied twice.
                self.run_plan(plan);
            }
            Err(e) => self.info(e),
        }
    }

    /// Assets were dropped on `target` (an object, or `None` for empty space) at `at` in the world.
    /// Each one that can be used is; the rest are explained.
    pub fn drop_assets(&mut self, paths: &[String], target: Option<Id>, at: Option<Vec3>) {
        if self.project.is_none() || self.play.is_some() {
            return;
        }
        let mut made = Vec::new();
        for path in paths {
            if self.asset_db().is_some_and(|db| db.has_folder(path)) {
                self.info(format!("{path} is a folder; drag the files in it."));
                continue;
            }
            match self.plan_asset(path, target, at, true) {
                Ok(plan) => made.extend(self.run_plan(plan)),
                Err(e) => self.warn(e),
            }
        }
        if made.len() > 1 {
            self.select_created(&made);
        }
    }

    /// Whether dropping these on `target` would work, for the feedback shown while dragging: the
    /// phrase for what happens, or why not.
    pub fn check_drop(&self, paths: &[String], target: Option<Id>) -> Result<String, String> {
        let mut verbs = Vec::new();
        for path in paths {
            if self.asset_db().is_some_and(|db| db.has_folder(path)) {
                return Err("Folders cannot be dropped into the scene.".to_owned());
            }
            let plan = self.plan_asset(path, target, None, true)?;
            verbs.push(plan.describe(&self.dm.borrow()));
        }
        verbs.dedup();
        Ok(if paths.len() > 1 { format!("{} ({} items)", verbs.first().cloned().unwrap_or_default(), paths.len()) } else { verbs.pop().unwrap_or_default() })
    }

    /// Saves the selected objects as a prefab in `Scenes/Prefabs` and selects it in the browser.
    pub fn save_selection_as_prefab(&mut self) {
        if !self.can_change_files() {
            return;
        }
        self.normalize_selection();
        let roots: Vec<Id> = {
            let dm = self.dm.borrow();
            self.top_level_selection().into_iter().filter(|id| dm.get(*id).is_some_and(|i| i.class.desc.service_order.is_none() && !i.class.desc.root)).collect()
        };
        let Some(first) = roots.first().copied() else { return self.warn("Select the objects to save as a prefab first (services cannot be saved).") };
        let Some(project) = &self.project else { return };
        let name = self.dm.borrow().name(first);
        let place = self.dm.borrow().to_prefab(&roots, &name);
        let dir = if project.root.join("Scenes").is_dir() || !project.root.join("scenes").is_dir() { project::PREFABS_DIR } else { "scenes/Prefabs" };
        let file = project.unique_file_name(dir, &project::sanitize_file_stem(&name, "Prefab"), project::kinds::PREFAB_EXT);
        let rel = join(dir, &file);
        match project.write_place(&rel, &place) {
            Ok(()) => {
                self.info(format!("Saved {} object(s) as {rel}", roots.len()));
                self.sync_assets(true);
                self.reveal_asset(&rel);
            }
            Err(e) => self.error(format!("{e:#}")),
        }
    }
}

impl OpenScript {
    pub fn new(path: String, text: String, stamp: Option<(u64, Option<SystemTime>)>) -> OpenScript {
        OpenScript { path, text, modified: false, stamp, seen: stamp, conflict: false, missing: false }
    }
}

impl Editor {
    /// The stamp of a project file as it is on disk now.
    pub(super) fn disk_stamp(&self, rel: &str) -> Option<(u64, Option<SystemTime>)> {
        self.project.as_ref().and_then(|p| p.abs(rel).ok()).and_then(|p| file_stamp(&p))
    }
}

#[cfg(test)]
mod tests {
    use forge_engine::project::Project;

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_assets_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Assets Test").expect("create project");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn editor(s: &Scratch) -> Editor {
        let mut ed = Editor::new(Some(s.0.clone()));
        ed.commit_edits(false);
        ed.history.clear();
        ed
    }

    fn find(ed: &Editor, name: &str) -> Id {
        let dm = ed.dm.borrow();
        dm.find_first_child(dm.workspace, name, true).unwrap_or_else(|| panic!("no '{name}'"))
    }

    fn source_of(ed: &Editor, id: Id) -> String {
        ed.dm.borrow().string(id, "SourceFile")
    }

    fn write(s: &Scratch, rel: &str, text: &str) {
        let path = s.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_new_project_has_the_recommended_layout_and_runs_from_it() {
        let s = Scratch::new();
        let ed = editor(&s);
        for dir in project::TEMPLATE_DIRS {
            assert!(s.0.join(dir).is_dir(), "{dir}");
        }
        for file in ["Project.toml", "README.md", "Scenes/MainScene.scene"] {
            assert!(s.0.join(file).is_file(), "{file}");
        }
        assert!(!s.0.join("forge.toml").exists());
        assert_eq!(ed.project.as_ref().unwrap().manifest.project.start_scene, "Scenes/MainScene.scene");
        assert_eq!(ed.scene_path.as_deref(), Some("Scenes/MainScene.scene"));
        let db = ed.asset_db().expect("scanned");
        assert!(db.entry("Server/Systems/Spin.luau").is_some() && db.entry("Client/Controllers/FirstPersonController.luau").is_some());
        // Scripts keep their class wherever their files are: the controller is a LocalScript, the spinner a Script.
        let dm = ed.dm.borrow();
        let controller = dm.find_first_child(dm.service("StarterPlayer").unwrap(), "FirstPersonController", false).unwrap();
        assert_eq!(dm.get(controller).unwrap().class.name, "LocalScript");
    }

    #[test]
    fn an_old_project_with_forge_toml_and_lowercase_folders_still_opens_and_keeps_its_layout() {
        let s = Scratch::new();
        // Rebuild the old layout by hand.
        std::fs::remove_dir_all(&s.0).unwrap();
        write(&s, "forge.toml", "[project]\nname = \"Old\"\nstart_scene = \"scenes/main.scene.json\"\n");
        write(&s, "scripts/hello.luau", "print('hi')");
        let place = forge_engine::dm::DataModel::new().to_place("Main");
        write(&s, "scenes/main.scene.json", &serde_json::to_string(&place).unwrap());
        let mut ed = Editor::new(Some(s.0.clone()));
        assert_eq!(ed.project.as_ref().unwrap().manifest_file, "forge.toml");
        assert_eq!(ed.scene_path.as_deref(), Some("scenes/main.scene.json"));
        assert_eq!(ed.scripts, ["scripts/hello.luau"]);
        ed.dirty = true;
        ed.save_all();
        assert!(s.0.join("forge.toml").is_file() && !s.0.join("Project.toml").exists(), "saves to the manifest it was opened from");
        // New things are created next to what is there.
        let made = ed.project.as_ref().unwrap().create_script(None, "Thing", "Script").unwrap();
        assert_eq!(made, "scripts/Thing.luau", "the old scripts folder is used when there is one");
    }

    #[test]
    fn a_script_made_in_the_editor_gets_a_file_named_after_it_in_the_usual_folder() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let target = ed.dm.borrow().service("ServerScriptService").unwrap();
        ed.select_only(Some(target));
        ed.insert("Script");
        let script = ed.selected.unwrap();
        assert_eq!(ed.dm.borrow().name(script), "Script");
        assert_eq!(source_of(&ed, script), "Server/Script.luau");
        assert!(std::fs::read_to_string(s.0.join("Server/Script.luau")).unwrap().contains("Hello from"), "it starts from the script template");

        let target = ed.dm.borrow().service("StarterPlayer").unwrap();
        ed.select_only(Some(target));
        ed.insert("LocalScript");
        assert_eq!(source_of(&ed, ed.selected.unwrap()), "Client/LocalScript.luau");
        let target = ed.dm.borrow().service("ReplicatedStorage").unwrap();
        ed.select_only(Some(target));
        ed.insert("ModuleScript");
        assert_eq!(source_of(&ed, ed.selected.unwrap()), "Shared/Modules/ModuleScript.luau");
        // A second script with the same name never overwrites the first.
        let target = ed.dm.borrow().service("ServerScriptService").unwrap();
        ed.select_only(Some(target));
        ed.insert("Script");
        assert_eq!(source_of(&ed, ed.selected.unwrap()), "Server/Script2.luau");
        assert!(ed.asset_db().unwrap().entry("Server/Script2.luau").is_some(), "and the browser lists it at once");

        // Optional folders are not required: with Server/ gone, it is simply made again.
        std::fs::remove_dir_all(s.0.join("Server")).unwrap();
        let target = ed.dm.borrow().service("ServerScriptService").unwrap();
        ed.select_only(Some(target));
        ed.insert("Script");
        assert_eq!(source_of(&ed, ed.selected.unwrap()), "Server/Script.luau");
    }

    #[test]
    fn renaming_a_script_renames_its_file_and_only_when_the_file_was_named_after_it() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let target = ed.dm.borrow().service("ServerScriptService").unwrap();
        ed.select_only(Some(target));
        ed.insert("Script");
        let script = ed.selected.unwrap();
        std::fs::write(s.0.join("Server/Script.luau"), "print('mine')").unwrap();

        ed.dm.borrow_mut().set_prop(script, "Name", Value::Str("Door".into())).unwrap();
        ed.queue_script_rename(script, "Script");
        // Still typing: nothing yet.
        ed.flush_script_renames(true);
        assert_eq!(source_of(&ed, script), "Server/Script.luau");
        ed.script_renames[0].2 -= Duration::from_secs(2);
        ed.flush_script_renames(false);
        assert_eq!(source_of(&ed, script), "Server/Door.luau");
        assert_eq!(std::fs::read_to_string(s.0.join("Server/Door.luau")).unwrap(), "print('mine')");
        assert!(!s.0.join("Server/Script.luau").exists());
        // The saved scene refers to the new file.
        let scene = ed.project.as_ref().unwrap().read_place(&ed.scene_path.clone().unwrap()).unwrap();
        assert!(scene.script_paths().iter().any(|(name, p)| name == "Door" && p == "Server/Door.luau"));

        // A script whose file has its own name keeps it.
        let spin = find(&ed, "Spin");
        let before = source_of(&ed, spin);
        ed.dm.borrow_mut().set_prop(spin, "Name", Value::Str("Spinner".into())).unwrap();
        ed.script_renamed(spin, "Spin");
        assert_ne!(source_of(&ed, spin), before, "its file was named after it, so it follows");
        assert_eq!(source_of(&ed, spin), "Server/Systems/Spinner.luau");
    }

    #[test]
    fn a_script_file_shared_by_several_objects_or_already_taken_is_left_alone() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        // The six coins all use Coin.luau: renaming one of them must not rename the shared file.
        let coin = find(&ed, "Collect");
        let file = source_of(&ed, coin);
        ed.dm.borrow_mut().set_prop(coin, "Name", Value::Str("Collector".into())).unwrap();
        ed.script_renamed(coin, "Collect");
        assert_eq!(source_of(&ed, coin), file);
        assert!(s.0.join(&file).is_file());

        // The name is already a file: keep the old name.
        write(&s, "Server/Taken.luau", "x");
        let target = ed.dm.borrow().service("ServerScriptService").unwrap();
        ed.select_only(Some(target));
        ed.insert("Script");
        let script = ed.selected.unwrap();
        ed.dm.borrow_mut().set_prop(script, "Name", Value::Str("Taken".into())).unwrap();
        ed.script_renamed(script, "Script");
        assert_eq!(source_of(&ed, script), "Server/Script.luau");
        assert_eq!(std::fs::read_to_string(s.0.join("Server/Taken.luau")).unwrap(), "x", "nothing was overwritten");
    }

    #[test]
    fn deleting_a_script_object_keeps_its_file_and_a_shared_file_stays_for_the_others() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let spin = find(&ed, "Spin");
        let file = source_of(&ed, spin);
        ed.delete(spin);
        assert!(s.0.join(&file).is_file(), "the file stays in the project");
        assert!(ed.asset_db().unwrap().entry(&file).is_some());
        // ... and is not offered for silent removal: deleting it is a separate, confirmed step.
        ed.asset_ask_delete(vec![file.clone()]);
        match &ed.dialog {
            Dialog::DeleteAssets { paths, used_by } => {
                assert_eq!(paths, &[file.clone()]);
                assert!(used_by.is_empty(), "the object using it is gone: {used_by:?}");
            }
            _ => panic!("expected the confirmation dialog"),
        }
        ed.dialog = Dialog::None;
        let coin_file = source_of(&ed, find(&ed, "Collect"));
        ed.asset_ask_delete(vec![coin_file]);
        match &ed.dialog {
            Dialog::DeleteAssets { used_by, .. } => assert!(used_by.iter().any(|u| u.ends_with(".scene")), "the dialog warns that the scene uses it: {used_by:?}"),
            _ => panic!("expected the confirmation dialog"),
        }
    }

    #[test]
    fn moving_a_script_keeps_the_scene_pointing_at_it_and_changes_nothing_about_where_it_runs() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let spin = find(&ed, "Spin");
        let class_before = ed.dm.borrow().get(spin).unwrap().class.name;
        let old = source_of(&ed, spin);
        let parent_before = ed.dm.borrow().parent(spin);

        ed.asset_new_folder();
        let folder = "Assets/New Folder";
        assert_eq!(ed.assets_ui.renaming.as_ref().map(|(p, _)| p.as_str()), Some(folder), "the new folder is being named");
        ed.finish_asset_rename(true);
        ed.asset_move(std::slice::from_ref(&old), folder);
        let new = format!("{folder}/Spin.luau");
        assert_eq!(source_of(&ed, spin), new, "the object follows the file");
        assert!(s.0.join(&new).is_file() && !s.0.join(&old).exists());
        assert_eq!((ed.dm.borrow().get(spin).unwrap().class.name, ed.dm.borrow().parent(spin)), (class_before, parent_before), "same class, same place in the scene: it still runs the same way");
        // Saved: the scene on disk has the new path (also for the coins' shared file).
        let scene = ed.project.as_ref().unwrap().read_place(&ed.scene_path.clone().unwrap()).unwrap();
        assert!(scene.script_paths().iter().any(|(n, p)| n == "Spin" && *p == new));
        assert!(forge_engine::project::export::validate(ed.project.as_ref().unwrap()).iter().all(|d| d.severity != forge_engine::project::export::Severity::Error));

        // Undo does not bring back a stale path: the history only holds the property edit.
        ed.commit_edits(false);
        let coin = find(&ed, "Collect");
        let coin_file = source_of(&ed, coin);
        ed.asset_rename(&coin_file, "Pickup.luau");
        assert_eq!(source_of(&ed, coin), join(parent_of(&coin_file), "Pickup.luau"));
        let coin_one = find(&ed, "Coin 1");
        let its_script = ed.dm.borrow().children(coin_one)[0];
        assert_eq!(source_of(&ed, its_script), join(parent_of(&coin_file), "Pickup.luau"), "every coin's script follows");
    }

    #[test]
    fn the_open_script_follows_a_move_and_syncs_with_its_file() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        ed.open_script("Server/Systems/Spin.luau");
        ed.script.as_mut().unwrap().text.push_str("-- edited\n");
        ed.script.as_mut().unwrap().modified = true;
        ed.save_all();
        assert!(std::fs::read_to_string(s.0.join("Server/Systems/Spin.luau")).unwrap().ends_with("-- edited\n"));
        ed.sync_assets(true);
        assert!(!ed.script.as_ref().unwrap().conflict, "our own save is not a change by someone else");

        // Changed from outside, no unsaved changes: reloaded.
        std::fs::write(s.0.join("Server/Systems/Spin.luau"), "-- from outside\n").unwrap();
        ed.sync_assets(true);
        assert_eq!(ed.script.as_ref().unwrap().text, "-- from outside\n");

        // Changed from outside while there are unsaved changes: flagged, nothing lost.
        ed.script.as_mut().unwrap().text.push_str("-- mine\n");
        ed.script.as_mut().unwrap().modified = true;
        std::fs::write(s.0.join("Server/Systems/Spin.luau"), "-- from outside again, longer\n").unwrap();
        ed.sync_assets(true);
        assert!(ed.script.as_ref().unwrap().conflict && ed.script.as_ref().unwrap().text.ends_with("-- mine\n"));

        // Moved: the editor keeps pointing at the file.
        ed.script.as_mut().unwrap().modified = false;
        ed.script.as_mut().unwrap().conflict = false;
        ed.asset_move(&["Server/Systems/Spin.luau".to_owned()], "Shared");
        assert_eq!(ed.script.as_ref().unwrap().path, "Shared/Spin.luau");
        // Deleted from outside: flagged as missing.
        std::fs::remove_file(s.0.join("Shared/Spin.luau")).unwrap();
        ed.sync_assets(true);
        assert!(ed.script.as_ref().unwrap().missing);
    }

    #[test]
    fn a_file_moved_outside_the_editor_is_noticed_and_the_scene_is_fixed() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let spin = find(&ed, "Spin");
        std::fs::create_dir_all(s.0.join("Gameplay")).unwrap();
        std::fs::rename(s.0.join("Server/Systems/Spin.luau"), s.0.join("Gameplay/Rotate.luau")).unwrap();
        ed.sync_assets(true);
        assert_eq!(source_of(&ed, spin), "Gameplay/Rotate.luau", "recognised by its contents, so the object follows");
        assert!(ed.log.iter().any(|l| l.text.contains("moved outside the editor")));
        let scene = ed.project.as_ref().unwrap().read_place(&ed.scene_path.clone().unwrap()).unwrap();
        assert!(scene.script_paths().iter().any(|(_, p)| p == "Gameplay/Rotate.luau"));
        ed.sync_assets(true);
        let n = ed.log.iter().filter(|l| l.text.contains("moved outside the editor")).count();
        ed.sync_assets(true);
        assert_eq!(ed.log.iter().filter(|l| l.text.contains("moved outside the editor")).count(), n, "no loop: the same move is not reported again");
    }

    #[test]
    fn folders_can_be_made_renamed_moved_duplicated_deleted_and_the_browser_follows() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        ed.browse_to("Assets");
        ed.asset_new_folder();
        assert!(s.0.join("Assets/New Folder").is_dir());
        ed.asset_new_folder();
        assert!(s.0.join("Assets/New Folder 2").is_dir(), "a free name each time");
        ed.finish_asset_rename(false);

        write(&s, "Assets/New Folder/crate.obj", "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
        ed.sync_assets(true);
        ed.browse_to("Assets/New Folder");
        ed.asset_rename("Assets/New Folder", "Props");
        assert_eq!(ed.assets_ui.dir, "Assets/Props", "the browser follows the folder it is in");
        assert!(s.0.join("Assets/Props/crate.obj").is_file());

        ed.asset_move(&["Assets/Props".to_owned()], "Assets/Models");
        assert!(s.0.join("Assets/Models/Props/crate.obj").is_file());
        ed.asset_move(&["Assets/Models".to_owned()], "Assets/Models/Props");
        assert!(!s.0.join("Assets/Models/Props/Models").exists(), "a folder cannot be moved into itself");
        ed.asset_duplicate(&["Assets/Models/Props".to_owned()]);
        assert!(s.0.join("Assets/Models/Props Copy/crate.obj").is_file());

        ed.asset_delete(&["Assets/Models/Props Copy".to_owned()]);
        assert!(!s.0.join("Assets/Models/Props Copy").exists());
        assert!(ed.asset_db().unwrap().entry("Assets/Models/Props Copy/crate.obj").is_none(), "the browser lists the change at once");
        let trash = std::fs::read_dir(s.0.join(".forge/trash")).unwrap().count();
        assert_eq!(trash, 1, "deleted, not destroyed");
        ed.browse_to("Assets/Models/Props");
        ed.asset_delete(&["Assets/Models/Props".to_owned()]);
        assert_eq!(ed.assets_ui.dir, "Assets/Models", "the folder being browsed was deleted: back to the one above");
    }

    #[test]
    fn the_open_scene_cannot_be_deleted_and_playing_blocks_file_changes() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        ed.asset_delete(&["Scenes".to_owned()]);
        assert!(s.0.join("Scenes/MainScene.scene").is_file(), "a folder holding the open scene is refused");
        ed.asset_delete(&["Scenes/MainScene.scene".to_owned()]);
        assert!(s.0.join("Scenes/MainScene.scene").is_file());
        ed.start_play();
        ed.asset_rename("Assets/Models", "Meshes");
        assert!(s.0.join("Assets/Models").is_dir(), "no file changes while a game is running");
        ed.stop_play();
    }

    #[test]
    fn importing_selects_the_new_files_and_failures_are_explained() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let outside = Scratch::new();
        std::fs::write(outside.0.join("good.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        std::fs::write(outside.0.join("bad.obj"), "nothing").unwrap();
        ed.browse_to("Assets/Models");
        ed.asset_import(vec![outside.0.join("good.obj"), outside.0.join("bad.obj"), outside.0.join("nope.obj")]);
        assert_eq!(ed.assets_ui.selected, ["Assets/Models/good.obj"]);
        assert!(ed.asset_db().unwrap().entry("Assets/Models/good.obj").is_some());
        assert!(ed.assets.contains(&"Assets/Models/good.obj".to_owned()), "the mesh picker offers it");
        let warnings: Vec<&str> = ed.log.iter().filter(|l| l.text.starts_with("could not import")).map(|l| l.text.as_str()).collect();
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    fn mesh_part(ed: &mut Editor, name: &str) -> Id {
        let mut dm = ed.dm.borrow_mut();
        let id = dm.create("MeshPart").unwrap();
        dm.set_name(id, name);
        let ws = dm.workspace;
        dm.set_parent(id, Some(ws)).unwrap();
        id
    }

    #[test]
    fn dropping_assets_on_objects_creates_or_assigns_and_says_why_when_it_cannot() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        write(&s, "Assets/Models/crate.obj", "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
        write(&s, "Assets/Models/car.fbx", "binary");
        write(&s, "Assets/Textures/brick.png", "png");
        write(&s, "Assets/Audio/boom.wav", "wav");
        write(&s, "Server/Util.luau", "return {}");
        ed.sync_assets(true);
        let (crate_obj, fbx, png, wav) = ("Assets/Models/crate.obj", "Assets/Models/car.fbx", "Assets/Textures/brick.png", "Assets/Audio/boom.wav");
        let wall = find(&ed, "Wall");
        let ws = ed.dm.borrow().workspace;

        // A mesh dropped on empty space or on a plain part creates a MeshPart; on a MeshPart it is assigned.
        assert!(matches!(ed.plan_asset(crate_obj, None, None, true), Ok(AssetPlan::CreateMesh { parent, .. }) if parent == ws));
        assert!(matches!(ed.plan_asset(crate_obj, Some(wall), None, true), Ok(AssetPlan::CreateMesh { parent, .. }) if parent == ws), "a part is not a container: the mesh goes next to it");
        let mp = mesh_part(&mut ed, "Holder");
        assert!(matches!(ed.plan_asset(crate_obj, Some(mp), None, true), Ok(AssetPlan::AssignMesh { target, .. }) if target == mp));
        ed.drop_assets(&[crate_obj.to_owned()], Some(mp), None);
        assert_eq!(ed.dm.borrow().string(mp, "MeshId"), crate_obj);
        assert!(ed.check_drop(&[crate_obj.to_owned()], Some(mp)).is_err(), "dropping the same mesh again would change nothing: say so, don't repeat it");
        let before = ed.dm.borrow().all_ids().len();
        ed.drop_assets(&[crate_obj.to_owned()], None, Some(Vec3::new(5.0, 1.0, 5.0)));
        assert_eq!(ed.dm.borrow().all_ids().len(), before + 1);
        let made = ed.selected.unwrap();
        assert_eq!((ed.dm.borrow().get(made).unwrap().class.name, ed.dm.borrow().name(made), ed.dm.borrow().string(made, "MeshId")), ("MeshPart", "crate".to_owned(), crate_obj.to_owned()));
        assert_eq!(ed.dm.borrow().vec3(made, "Position"), Vec3::new(5.0, 1.0, 5.0));

        // What cannot be used says so.
        assert!(ed.check_drop(&[fbx.to_owned()], None).unwrap_err().contains("glTF"));
        assert!(ed.check_drop(&[png.to_owned()], None).unwrap_err().contains("Decal"), "a picture needs a part or a decal to go on");
        assert!(ed.check_drop(&[wav.to_owned()], None).is_ok(), "a sound dropped on empty space becomes a Sound object");
        assert!(ed.check_drop(&["Scenes/MainScene.scene".to_owned()], None).unwrap_err().contains("prefab"));
        assert!(ed.check_drop(&["Assets".to_owned()], None).is_err());
        let count = ed.dm.borrow().all_ids().len();
        ed.drop_assets(&[fbx.to_owned(), "Assets/Models/missing.dae".to_owned()], Some(wall), None);
        assert_eq!(ed.dm.borrow().all_ids().len(), count, "nothing is created for what cannot be used");
    }

    #[test]
    fn dropping_a_script_adds_one_script_object_of_a_fitting_class_or_assigns_it() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        write(&s, "Anywhere/Util.luau", "return {}");
        ed.sync_assets(true);
        let util = "Anywhere/Util.luau";
        let (wall, sps, starter, rs) = {
            let dm = ed.dm.borrow();
            (dm.find_first_child(dm.workspace, "Wall", true).unwrap(), dm.service("ServerScriptService").unwrap(), dm.service("StarterPlayer").unwrap(), dm.service("ReplicatedStorage").unwrap())
        };
        let class_at = |ed: &Editor, t: Id| match ed.plan_asset(util, Some(t), None, true) {
            Ok(AssetPlan::CreateScript { class, parent, .. }) => (class, parent),
            other => panic!("{other:?}"),
        };
        assert_eq!(class_at(&ed, wall), ("Script", wall), "a Script on a part, like the Spin script");
        assert_eq!(class_at(&ed, sps).0, "Script");
        assert_eq!(class_at(&ed, starter).0, "LocalScript");
        assert_eq!(class_at(&ed, rs).0, "ModuleScript");

        ed.drop_assets(&[util.to_owned()], Some(wall), None);
        let added = ed.selected.unwrap();
        assert_eq!((ed.dm.borrow().parent(added), source_of(&ed, added)), (Some(wall), util.to_owned()));
        assert!(ed.check_drop(&[util.to_owned()], Some(wall)).is_err(), "no second object for the same file on the same part");
        let n = ed.dm.borrow().all_ids().len();
        ed.drop_assets(&[util.to_owned()], Some(wall), None);
        assert_eq!(ed.dm.borrow().all_ids().len(), n, "dropping again adds nothing");
        // Dropped on a script object: assigned to it; the file's own class is kept for later drops.
        write(&s, "Anywhere/Other.luau", "print(1)");
        ed.sync_assets(true);
        assert!(matches!(ed.plan_asset("Anywhere/Other.luau", Some(added), None, true), Ok(AssetPlan::AssignScript { target, .. }) if target == added));
        assert_eq!(class_at_for(&ed, util, rs), "Script", "the class of an object that already uses the file wins over the service");
    }

    fn class_at_for(ed: &Editor, path: &str, target: Id) -> &'static str {
        match ed.plan_asset(path, Some(target), None, true) {
            Ok(AssetPlan::CreateScript { class, .. }) => class,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn double_clicking_uses_the_selection_and_never_duplicates() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        write(&s, "Assets/Models/crate.obj", "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
        ed.sync_assets(true);
        let crate_obj = "Assets/Models/crate.obj";

        // Nothing suitable selected: a new object. Selected MeshPart: it is used, once.
        let before = ed.dm.borrow().all_ids().len();
        ed.open_asset(crate_obj);
        assert_eq!(ed.dm.borrow().all_ids().len(), before + 1);
        let first = ed.selected.unwrap();
        ed.open_asset(crate_obj);
        assert_eq!(ed.dm.borrow().all_ids().len(), before + 1, "the MeshPart that is selected already shows it: nothing new");
        assert!(ed.log.iter().any(|l| l.text.contains("already shows")), "and the editor says why");
        let _ = first;

        // Folders navigate, scripts open in the editor, scenes load, textures preview, the rest explains.
        ed.open_asset("Assets/Models");
        assert_eq!(ed.assets_ui.dir, "Assets/Models");
        ed.open_asset("Server/Services/GameManager.luau");
        assert_eq!(ed.script.as_ref().unwrap().path, "Server/Services/GameManager.luau");
        write(&s, "Assets/Textures/brick.png", "png");
        ed.sync_assets(true);
        ed.open_asset("Assets/Textures/brick.png");
        assert_eq!(ed.assets_ui.preview.as_deref(), Some("Assets/Textures/brick.png"));
        ed.new_scene("Second");
        ed.open_asset("Scenes/MainScene.scene");
        assert_eq!(ed.scene_path.as_deref(), Some("Scenes/MainScene.scene"));
    }

    #[test]
    fn a_prefab_saves_the_selection_and_places_a_copy_with_its_references_intact() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = find(&ed, "Player");
        ed.select_only(Some(model));
        ed.save_selection_as_prefab();
        assert!(s.0.join("Scenes/Prefabs/Player.prefab").is_file());
        assert_eq!(ed.assets_ui.selected, ["Scenes/Prefabs/Player.prefab"]);
        assert_eq!(ed.assets_ui.dir, "Scenes/Prefabs", "revealed in the browser");

        let before = ed.dm.borrow().all_ids().len();
        ed.drop_assets(&["Scenes/Prefabs/Player.prefab".to_owned()], None, None);
        let copy = ed.selected.unwrap();
        assert_ne!(copy, model);
        let dm = ed.dm.borrow();
        assert_eq!(dm.all_ids().len(), before + 2, "the model and its body part");
        let root = dm.find_child(copy, "Body").expect("the copy has its parts");
        assert_eq!(dm.reference(copy, "PrimaryPart"), Some(root), "references inside the prefab point at the copy, not the original");
        drop(dm);
        assert!(forge_engine::project::export::validate(ed.project.as_ref().unwrap()).iter().all(|d| d.severity != forge_engine::project::export::Severity::Error), "a prefab is a valid file");
        // Services cannot be saved as prefabs.
        let ws = ed.dm.borrow().workspace;
        ed.select_only(Some(ws));
        ed.save_selection_as_prefab();
        assert!(!s.0.join("Scenes/Prefabs/Workspace.prefab").exists());
    }

    #[test]
    fn a_reimported_mesh_asks_the_viewport_to_reload_and_a_missing_one_is_reported() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        write(&s, "Assets/Models/crate.obj", "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
        ed.sync_assets(true);
        ed.asset_reimport("Assets/Models/crate.obj");
        assert!(ed.reload_meshes.get());
        ed.asset_reimport("Assets/Models/gone.obj");
        assert!(ed.log.iter().any(|l| l.text.contains("no longer exists")));
    }

    #[test]
    fn grid_selection_follows_ctrl_and_shift() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let order: Vec<String> = ["a", "b", "c", "d", "e"].iter().map(|x| x.to_string()).collect();
        ed.click_asset(&order, "b", false, false);
        ed.click_asset(&order, "d", false, true);
        assert_eq!(ed.assets_ui.selected, ["b", "c", "d"]);
        ed.click_asset(&order, "a", false, true);
        assert_eq!(ed.assets_ui.selected, ["a", "b"], "from the anchor, which did not move");
        ed.click_asset(&order, "e", true, false);
        ed.click_asset(&order, "b", true, false);
        assert_eq!(ed.assets_ui.selected, ["a", "e"]);
        ed.click_asset(&order, "gone", false, true);
        assert_eq!(ed.assets_ui.selected, ["gone"], "a range to something that is not listed selects just that");
    }
}