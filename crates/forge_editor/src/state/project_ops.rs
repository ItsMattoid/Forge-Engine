//! Projects, scenes, scripts and saving.

use std::{fs, path::Path};

use forge_engine::{
    assets::GameSource,
    project::{self, write_atomic, Project},
};

use super::{Editor, Level, OpenScript};
use crate::dock::Panel;

impl Editor {
    /// Looks at the project's files again (the scenes, scripts and assets, and the Assets tab).
    pub fn refresh_files(&mut self) {
        if let Some(p) = &self.project {
            self.source = GameSource::Dir(p.root.clone());
        }
        self.sync_assets(true);
    }

    /// A project was opened or created: the Assets tab starts from scratch, in the first folder worth showing.
    fn reset_asset_browser(&mut self) {
        self.assets_ui = super::AssetBrowser::default();
        self.refresh_files();
        let start = self.asset_db().filter(|db| db.has_folder(project::ASSETS_DIR)).map(|_| project::ASSETS_DIR.to_owned()).unwrap_or_default();
        self.browse_to(&start);
    }

    pub fn open_project(&mut self, path: &Path) {
        self.stop_play();
        self.forget_duplicate();
        self.save_all();
        match Project::open(path) {
            Ok(project) => {
                let start = project.manifest.project.start_scene.clone();
                self.info(format!("Opened project '{}'", project.name()));
                self.project = Some(project);
                self.script = None;
                self.reset_asset_browser();
                self.load_scene(&start);
            }
            Err(e) => self.error(format!("{e:#}")),
        }
    }

    pub fn create_project(&mut self, dir: &Path, name: &str) {
        self.stop_play();
        self.save_all();
        match Project::create(dir, name) {
            Ok(project) => {
                self.info(format!("Created project '{}' in {}", project.name(), dir.display()));
                let start = project.manifest.project.start_scene.clone();
                self.project = Some(project);
                self.script = None;
                self.reset_asset_browser();
                self.load_scene(&start);
            }
            Err(e) => self.error(format!("{e:#}")),
        }
    }

    pub fn load_scene(&mut self, rel: &str) {
        self.stop_play();
        self.forget_duplicate();
        self.save_all();
        let Some(project) = &self.project else { return };
        let place = match project.read_place(rel) {
            Ok(p) => p,
            Err(e) => return self.error(format!("{e:#}")),
        };
        // An unsupported format is refused before anything is touched, so a failed load leaves
        // the open scene (and `scene_path`) exactly as they were.
        let loaded = self.dm.borrow_mut().load_place(&place);
        if let Err(e) = loaded {
            return self.error(format!("{rel}: {e}"));
        }
        let warnings = std::mem::take(&mut self.dm.borrow_mut().load_warnings);
        for w in warnings {
            self.warn(format!("{rel}: {w}"));
        }
        self.scene_name = if place.name.is_empty() { "Main".into() } else { place.name };
        self.scene_path = Some(rel.to_owned());
        self.selected = None;
        self.selection.clear();
        self.sel_committed = super::SelectionState::default();
        self.history.clear();
        self.dirty = false;
        self.info(format!("Loaded {rel}"));
    }

    pub fn save_all(&mut self) {
        let mut messages: Vec<(Level, String)> = Vec::new();
        // While a game is playing the data model holds the running game, not the scene being
        // edited: the scene is not written then (Stop restores it).
        let playing = self.play.is_some();
        if let Some(project) = &self.project {
            if self.dirty {
                if let Some(rel) = self.scene_path.as_ref().filter(|_| !playing) {
                    match project.write_place(rel, &self.dm.borrow().to_place(&self.scene_name)) {
                        Ok(()) => messages.push((Level::Info, format!("Saved {rel}"))),
                        Err(e) => messages.push((Level::Error, format!("{e:#}"))),
                    }
                }
                if let Err(e) = project.save_manifest() {
                    messages.push((Level::Error, format!("{e:#}")));
                }
            }
            if let Some(s) = &mut self.script {
                if s.modified {
                    match project.abs(&s.path).and_then(|p| write_atomic(&p, &s.text)) {
                        Ok(()) => {
                            s.modified = false;
                            s.conflict = false;
                            s.missing = false;
                            s.stamp = project.abs(&s.path).ok().and_then(|p| std::fs::metadata(p).ok()).map(|m| (m.len(), m.modified().ok()));
                            messages.push((Level::Info, format!("Saved {}", s.path)));
                        }
                        Err(e) => messages.push((Level::Error, format!("{e:#}"))),
                    }
                }
            }
        }
        if !playing {
            self.dirty = false;
        }
        for (l, m) in messages {
            self.push_log(l, m);
        }
    }

    /// Writes the project manifest (`Project.toml`) now.
    pub fn save_project_manifest(&mut self) {
        self.project_settings_ui.pending_save = false;
        let Some(project) = &self.project else { return };
        if let Err(e) = project.save_manifest() {
            self.error(format!("{e:#}"));
        }
    }

    // ---- the game's icon and splash screen ---------------------------------------------------------------

    /// The project file now chosen as the icon or the splash screen.
    pub fn launch_image(&self, role: project::launch::Role) -> Option<&str> {
        let launch = &self.project.as_ref()?.manifest.launch;
        match role {
            project::launch::Role::Icon => launch.icon.as_deref(),
            project::launch::Role::Splash => launch.splash.as_deref(),
        }
    }

    /// The project's files that could be an icon or a splash screen (by their name: PNG and JPEG).
    pub fn asset_files_with_image_extension(&self) -> Vec<String> {
        self.assets.iter().filter(|path| project::launch::has_supported_extension(path)).cloned().collect()
    }

    fn set_launch_file(&mut self, role: project::launch::Role, rel: Option<String>) {
        let Some(project) = self.project.as_mut() else { return };
        let launch = &mut project.manifest.launch;
        match role {
            project::launch::Role::Icon => launch.icon = rel,
            project::launch::Role::Splash => launch.splash = rel,
        }
        self.project_settings_ui.pending_save = true;
    }

    /// Chooses a picture from anywhere on disk as the icon or the splash screen. It is checked first (a format the
    /// engine cannot read, a broken or too small image are refused with the reason, and what was chosen before stays);
    /// then a file that is not in the project yet is copied into `Assets/Textures`, so what the manifest keeps is the
    /// picture's place in the project - a file that is packed with the game and follows the file if it is moved - and
    /// never the path it happened to be picked from.
    pub fn choose_launch_image(&mut self, role: project::launch::Role, source: &Path) {
        self.project_settings_ui.launch_message = None;
        let Some(project) = &self.project else { return };
        let name = source.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
        let checked = fs::read(source).map_err(|e| format!("'{}' could not be read: {e}", source.display())).and_then(|bytes| project::launch::check_image(&name, &bytes, role));
        if let Err(why) = checked {
            self.project_settings_ui.launch_message = Some((role, why));
            return;
        }
        // Already in the project (and not in a folder the editor keeps to itself): refer to it where it is.
        let inside = source.strip_prefix(&project.root).ok().map(|sub| sub.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")).filter(|rel| !rel.split('/').any(|part| part.starts_with('.')));
        let rel = match inside {
            Some(rel) => rel,
            None => {
                let imported = project.import_files(&[source.to_path_buf()], &format!("{}/Textures", project::ASSETS_DIR));
                match imported.into_iter().next() {
                    Some((_, Ok(rel))) => rel,
                    Some((_, Err(why))) => {
                        self.project_settings_ui.launch_message = Some((role, format!("could not copy '{name}' into the project: {why}")));
                        return;
                    }
                    None => return,
                }
            }
        };
        self.sync_assets(true);
        self.info(format!("The game's {} is now {rel}.", role.label()));
        self.set_launch_file(role, Some(rel));
    }

    /// Chooses a picture that is already in the project (from the page's list of them).
    pub fn choose_launch_file(&mut self, role: project::launch::Role, rel: &str) {
        self.project_settings_ui.launch_message = None;
        let Some(project) = &self.project else { return };
        match project::launch::check_project_file(project, rel, role) {
            Ok(_) => self.set_launch_file(role, Some(rel.to_owned())),
            Err(why) => self.project_settings_ui.launch_message = Some((role, why)),
        }
    }

    /// Takes the icon or the splash screen away (the game then starts with the default icon, or without a splash screen).
    pub fn clear_launch_image(&mut self, role: project::launch::Role) {
        self.project_settings_ui.launch_message = None;
        self.set_launch_file(role, None);
    }

    /// Writes only the script being edited (what Play needs on disk), leaving the scene alone.
    pub fn save_open_script(&mut self) {
        let Some(project) = &self.project else { return };
        let Some(s) = self.script.as_mut().filter(|s| s.modified) else { return };
        match project.abs(&s.path).and_then(|p| write_atomic(&p, &s.text)) {
            Ok(()) => {
                s.modified = false;
                s.conflict = false;
                s.missing = false;
                s.stamp = project.abs(&s.path).ok().and_then(|p| std::fs::metadata(p).ok()).map(|m| (m.len(), m.modified().ok()));
                let path = s.path.clone();
                self.info(format!("Saved {path}"));
            }
            Err(e) => self.error(format!("{e:#}")),
        }
    }

    pub fn open_script(&mut self, rel: &str) {
        if self.project.is_none() {
            return;
        }
        if self.script.as_ref().is_some_and(|s| s.modified && s.path != rel) {
            self.save_all();
        }
        let Some(project) = &self.project else { return };
        match project.abs(rel).and_then(|p| Ok(fs::read_to_string(p)?)) {
            Ok(text) => {
                let stamp = self.disk_stamp(rel);
                self.script = Some(OpenScript::new(rel.to_owned(), text, stamp));
                self.show_panel(Panel::ScriptEditor);
            }
            Err(e) => self.error(format!("{e:#}")),
        }
    }

    pub fn new_scene(&mut self, name: &str) {
        let Some(project) = &self.project else { return };
        // In the folder being browsed when that is not the project's own assets, else the usual Scenes folder.
        let dir = self.assets_ui.dir.clone();
        let made = if dir.is_empty() || dir == project::ASSETS_DIR || dir.starts_with(&format!("{}/", project::ASSETS_DIR)) { project.new_scene(name) } else { project.new_scene_in(&dir, name) };
        match made {
            Ok(rel) => {
                self.refresh_files();
                self.load_scene(&rel);
            }
            Err(e) => self.error(format!("{e:#}")),
        }
    }
}
