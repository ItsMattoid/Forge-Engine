//! A Forge project on disk: the folder, its manifest and the files in it.
//!
//! * this file    - [`Project`]: the folder, discovering what is in it, creating scenes and scripts
//! * [`kinds`]    - what a file is, by its name ([`AssetKind`])
//! * [`files`]    - safe file operations (rename, move, duplicate, delete, import) that keep references intact
//! * [`assetdb`]  - the index of the project's files: stable ids, kinds, what refers to what
//! * [`manifest`] - `Project.toml`
//! * [`template`] - starter content for new projects
//! * [`export`]   - validation and packaging into a standalone game
//! * [`launcher`] - finding and starting the player / editor executables
//!
//! A new project starts from this layout. It is a template, not a rule: every folder is optional, and
//! files can be put (and moved) anywhere inside the project.
//!
//! ```text
//! MyGame/
//!   Assets/        Models/ Textures/ Materials/ Audio/ Animations/ Fonts/
//!   Scenes/        MainScene.scene   Prefabs/
//!   Client/        Controllers/ UI/ Input/
//!   Server/        Services/ Systems/
//!   Shared/        Modules/ Types/ Constants/
//!   Packages/  Plugins/
//!   Project.toml   README.md
//! ```
//!
//! Where a script *file* lives never decides where the script *runs*: that is the class of the Script
//! instance (`Script`, `LocalScript`, `ModuleScript`) and where it sits in the scene. An instance only
//! stores the path of its source file. Projects made before this layout (`forge.toml`, `scenes/`,
//! `scripts/`, `assets/`) open and run unchanged.

pub mod assetdb;
pub mod export;
pub mod files;
pub mod kinds;
pub mod launch;
pub mod launcher;
pub mod manifest;
pub mod player_settings;
pub mod template;

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{bail, ensure, Context, Result};
use walkdir::WalkDir;

pub use assetdb::{AssetDb, AssetEntry, SearchHit, SearchQuery, SyncReport};
pub use kinds::{file_name, join, parent_of, stem_of, AssetKind};
pub use manifest::{AssetSettings, DisplayMode, GameSettings, GameType, LaunchSettings, MultiplayerSettings, ProjectInfo, ProjectManifest, QualityPreset, ShadowQuality, WindowSettings};

use crate::{assets::clean_path, dm::file::PlaceFile};

/// The project manifest.
pub const MANIFEST: &str = "Project.toml";
/// The manifest of projects made before `Project.toml`.
pub const LEGACY_MANIFEST: &str = "forge.toml";
pub const SCENES_DIR: &str = "Scenes";
pub const ASSETS_DIR: &str = "Assets";
pub const PREFABS_DIR: &str = "Scenes/Prefabs";
/// The editor's own bookkeeping (the asset index, deleted files). Never part of a build.
pub const META_DIR: &str = ".forge";
pub const SCENE_EXT: &str = kinds::SCENE_EXT;

/// The folders a new project starts with (all optional afterwards).
pub const TEMPLATE_DIRS: &[&str] = &[
    "Assets/Models",
    "Assets/Textures",
    "Assets/Materials",
    "Assets/Audio",
    "Assets/Animations",
    "Assets/Fonts",
    "Scenes/Prefabs",
    "Client/Controllers",
    "Client/UI",
    "Client/Input",
    "Server/Services",
    "Server/Systems",
    "Shared/Modules",
    "Shared/Types",
    "Shared/Constants",
    "Packages",
    "Plugins",
];

#[derive(Clone, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: ProjectManifest,
    /// Which file the manifest is saved to: `Project.toml`, or `forge.toml` for an older project.
    pub manifest_file: &'static str,
}

/// A file found in the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    /// Project-relative, `/`-separated.
    pub path: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// Writes a file so that a crash or a full disk can never leave it half-written: the data goes
/// to a temporary file next to it, which then replaces the original in one step.
pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> Result<()> {
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    if let Err(e) = fs::write(&tmp, bytes.as_ref()).and_then(|()| fs::rename(&tmp, path)) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing {}", path.display()));
    }
    Ok(())
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_owned();
    if out.is_empty() {
        "game".into()
    } else {
        out
    }
}

/// A name that is safe to use as a file or folder name on any system: characters the file system
/// rejects become `_`, and spaces and dots at the ends go. Never empty (`fallback` then).
pub fn sanitize_file_stem(name: &str, fallback: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_owned();
    if cleaned.is_empty() {
        fallback.to_owned()
    } else {
        cleaned
    }
}

fn rel_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether a directory is left out of the project's content: hidden folders (`.git`, `.forge`) and the
/// folders builds and tools put next to the project.
fn skipped_dir(depth: usize, name: &str) -> bool {
    name.starts_with('.') || (depth == 1 && matches!(name, "build" | "target" | "node_modules"))
}

impl Project {
    /// Creates a new project in `root` (which must not already be a project).
    pub fn create(root: &Path, name: &str) -> Result<Self> {
        ensure!(!root.join(MANIFEST).exists() && !root.join(LEGACY_MANIFEST).exists(), "{} already contains a Forge project", root.display());
        fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
        for dir in TEMPLATE_DIRS {
            fs::create_dir_all(root.join(dir)).with_context(|| format!("creating {}", root.join(dir).display()))?;
        }
        let manifest = ProjectManifest { project: ProjectInfo { name: name.trim().to_owned(), ..ProjectInfo::default() }, ..ProjectManifest::default() };
        let project = Self { root: root.to_owned(), manifest, manifest_file: MANIFEST };
        project.save_manifest()?;
        template::populate(&project)?;
        Ok(project)
    }

    pub fn open(root: &Path) -> Result<Self> {
        for file in [MANIFEST, LEGACY_MANIFEST] {
            let path = root.join(file);
            if path.is_file() {
                let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
                return Ok(Self { root: root.to_owned(), manifest: ProjectManifest::parse(&bytes)?, manifest_file: file });
            }
        }
        bail!("{} is not a Forge project (missing {MANIFEST})", root.display())
    }

    pub fn save_manifest(&self) -> Result<()> {
        write_atomic(&self.root.join(self.manifest_file), toml::to_string_pretty(&self.manifest)?)
    }

    pub fn name(&self) -> &str {
        &self.manifest.project.name
    }

    pub fn slug(&self) -> String {
        slugify(self.name())
    }

    pub fn abs(&self, rel: &str) -> Result<PathBuf> {
        Ok(self.root.join(clean_path(rel)?))
    }

    // ---- discovery -----------------------------------------------------------------------------

    /// Every folder and file of the project (not the hidden or build folders), sorted by path. This
    /// is the one place that walks the disk: the lists below, the asset index and packaging all use it.
    pub fn walk(&self) -> (Vec<String>, Vec<FileInfo>) {
        let mut folders = Vec::new();
        let mut files = Vec::new();
        let walker = WalkDir::new(&self.root).min_depth(1).follow_links(false).into_iter().filter_entry(|e| !(e.file_type().is_dir() && skipped_dir(e.depth(), &e.file_name().to_string_lossy())));
        for entry in walker.filter_map(Result::ok) {
            let rel = rel_string(&self.root, entry.path());
            if entry.file_type().is_dir() {
                folders.push(rel);
            } else if entry.file_type().is_file() {
                let (size, modified) = entry.metadata().map(|m| (m.len(), m.modified().ok())).unwrap_or((0, None));
                files.push(FileInfo { path: rel, size, modified });
            }
        }
        folders.sort();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        (folders, files)
    }

    fn is_manifest(rel: &str) -> bool {
        rel == MANIFEST || rel == LEGACY_MANIFEST
    }

    fn files_of(&self, wanted: impl Fn(&str, AssetKind) -> bool) -> Vec<String> {
        self.walk().1.into_iter().map(|f| f.path).filter(|p| wanted(p, AssetKind::of(file_name(p)))).collect()
    }

    /// Every scene file, wherever it is.
    pub fn scenes(&self) -> Vec<String> {
        self.files_of(|_, k| k == AssetKind::Scene)
    }

    /// Every Luau source file, wherever it is.
    pub fn scripts(&self) -> Vec<String> {
        self.files_of(|_, k| k == AssetKind::Script)
    }

    pub fn prefabs(&self) -> Vec<String> {
        self.files_of(|_, k| k == AssetKind::Prefab)
    }

    /// Every file that is not a scene, a prefab, a script or the manifest (meshes, textures, audio, ...).
    pub fn asset_files(&self) -> Vec<String> {
        self.files_of(|p, k| !Self::is_manifest(p) && !matches!(k, AssetKind::Scene | AssetKind::Script | AssetKind::Prefab))
    }

    /// Everything a build contains besides the manifest: all files of the project except the root README.
    pub fn packaged_files(&self) -> Vec<String> {
        self.files_of(|p, _| !Self::is_manifest(p) && !p.eq_ignore_ascii_case("README.md"))
    }

    pub fn read_place(&self, rel: &str) -> Result<PlaceFile> {
        let bytes = fs::read(self.abs(rel)?).with_context(|| format!("reading {rel}"))?;
        serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes)).with_context(|| format!("parsing {rel}"))
    }

    pub fn write_place(&self, rel: &str, place: &PlaceFile) -> Result<()> {
        let path = self.abs(rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(&path, serde_json::to_string_pretty(place)?)
    }

    /// The first of `candidates` (project-relative folders) that exists, else the first one.
    fn existing_dir(&self, candidates: &[&str]) -> String {
        candidates.iter().find(|c| self.root.join(c).is_dir()).unwrap_or(&candidates[0]).to_string()
    }

    // ---- creating scenes and scripts -----------------------------------------------------------

    /// A free `stem.ext` (then `stem2.ext`, `stem3.ext`, ...) in `dir`.
    pub fn unique_file_name(&self, dir: &str, stem: &str, ext: &str) -> String {
        let mut n = 1;
        loop {
            let candidate = if n == 1 { format!("{stem}{ext}") } else { format!("{stem}{n}{ext}") };
            if !self.root.join(join(dir, &candidate)).exists() {
                return candidate;
            }
            n += 1;
        }
    }

    /// Creates `Scenes/<Name>.scene` with a baseplate and returns its path.
    pub fn new_scene(&self, name: &str) -> Result<String> {
        let dir = self.existing_dir(&[SCENES_DIR, "scenes"]);
        self.new_scene_in(&dir, name)
    }

    /// Creates `<dir>/<Name>.scene` with a baseplate and returns its path.
    pub fn new_scene_in(&self, dir: &str, name: &str) -> Result<String> {
        let dir = clean_path(dir)?.trim_end_matches('/').to_owned();
        let file = format!("{}{SCENE_EXT}", sanitize_file_stem(name, "Scene"));
        let rel = join(&dir, &file);
        ensure!(!self.abs(&rel)?.exists(), "{rel} already exists");
        self.write_place(&rel, &template::empty_place(name.trim()))?;
        Ok(rel)
    }

    /// Where a new script of this class goes by default. Only a suggestion: it can be moved anywhere
    /// without changing what it does (the class decides that, not the folder).
    pub fn script_dir_for(&self, class: &str) -> String {
        match class {
            "LocalScript" => self.existing_dir(&["Client", "scripts"]),
            "ModuleScript" => self.existing_dir(&["Shared/Modules", "Shared", "scripts"]),
            _ => self.existing_dir(&["Server", "scripts"]),
        }
    }

    /// Creates a Luau file for a new script of `class` called `name` (its file is named after it) in
    /// `dir`, or in the class's default folder, and returns its path.
    pub fn create_script(&self, dir: Option<&str>, name: &str, class: &str) -> Result<String> {
        let dir = match dir {
            Some(d) => clean_path(d)?.trim_end_matches('/').to_owned(),
            None => self.script_dir_for(class),
        };
        let stem = sanitize_file_stem(name.trim().trim_end_matches(".luau"), class);
        let rel = join(&dir, &self.unique_file_name(&dir, &stem, kinds::SCRIPT_EXT));
        let path = self.abs(&rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(&path, template::script_stub(class))?;
        Ok(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("My Cool Game!"), "my_cool_game");
        assert_eq!(slugify("***"), "game");
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(sanitize_file_stem("Spin: fast?", "x"), "Spin_ fast_");
        assert_eq!(sanitize_file_stem("  ..  ", "Script"), "Script");
        assert_eq!(sanitize_file_stem("a/b\\c", "x"), "a_b_c");
        assert_eq!(sanitize_file_stem("Coin Collector", "x"), "Coin Collector");
    }
}
