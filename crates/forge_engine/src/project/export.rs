//! Project validation and standalone-game packaging.
//!
//! A build produces a folder containing the player executable (renamed after the
//! game) next to a `game.pak` with the manifest, scenes, scripts and assets:
//!
//! ```text
//! build/my_game/
//!   my_game(.exe)
//!   game.pak
//! ```

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, ensure, Context, Result};

use super::{file_name, launch, AssetKind, Project, MANIFEST};
use crate::{
    assets::{PakBuilder, PAK_NAME},
    dm::file::FORMAT,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    fn err(m: impl Into<String>) -> Self {
        Self { severity: Severity::Error, message: m.into() }
    }
    fn warn(m: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, message: m.into() }
    }
}

pub fn has_errors(d: &[Diagnostic]) -> bool {
    d.iter().any(|d| d.severity == Severity::Error)
}

fn check_script(rel: &str, src: &str) -> Option<String> {
    let lua = mlua::Lua::new();
    lua.load(src).set_name(format!("={rel}")).into_function().err().map(|e| format!("{rel}: {e}"))
}

/// Checks the manifest, every scene and every script without running anything.
pub fn validate(project: &Project) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let start = &project.manifest.project.start_scene;
    let scenes = project.scenes();

    if !scenes.iter().any(|s| s == start) {
        out.push(Diagnostic::err(format!("start scene '{start}' does not exist")));
    }

    // The icon and the splash screen the manifest names must be there and be usable images.
    for (file, role) in [(project.manifest.launch.icon.as_deref(), launch::Role::Icon), (project.manifest.launch.splash.as_deref(), launch::Role::Splash)] {
        let Some(file) = file else { continue };
        match launch::check_project_file(project, file, role) {
            Ok(info) => out.extend(launch::advice(info, role).map(Diagnostic::warn)),
            Err(e) => out.push(Diagnostic::err(format!("launch settings: {e}"))),
        }
    }

    for rel in project.scripts() {
        match fs::read_to_string(project.root.join(&rel)) {
            Ok(src) => {
                if let Some(e) = check_script(&rel, &src) {
                    out.push(Diagnostic::err(format!("syntax error in {e}")));
                }
            }
            Err(e) => out.push(Diagnostic::err(format!("cannot read {rel}: {e}"))),
        }
    }

    for rel in scenes.iter().chain(project.prefabs().iter()) {
        let place = match project.read_place(rel) {
            Ok(p) => p,
            Err(e) => {
                out.push(Diagnostic::err(format!("{e:#}")));
                continue;
            }
        };
        if place.format != FORMAT {
            out.push(Diagnostic::err(format!("{rel}: scene format {} is not supported (expected {FORMAT}); recreate the scene", place.format)));
            continue;
        }
        // Loading it into a data model catches unknown classes and bad properties. (Loading
        // itself is forgiving, so what it had to skip is reported here.)
        let mut dm = crate::dm::DataModel::new();
        match dm.load_place(&place) {
            Ok(()) => {
                for w in &dm.load_warnings {
                    out.push(Diagnostic::err(format!("{rel}: {w}")));
                }
            }
            Err(e) => out.push(Diagnostic::err(format!("{rel}: {e}"))),
        }
        for (name, script) in place.script_paths() {
            if !project.abs(&script).map(|p| p.is_file()).unwrap_or(false) {
                out.push(Diagnostic::err(format!("{rel}: '{name}' references missing script '{script}'")));
            }
        }
        for (name, asset) in place.asset_paths() {
            if !project.abs(&asset).map(|p| p.is_file()).unwrap_or(false) {
                out.push(Diagnostic::warn(format!("{rel}: '{name}' references missing asset '{asset}' (a box is shown instead)")));
            }
        }
    }
    out
}

pub struct BuildOptions {
    pub out_dir: PathBuf,
    pub player: PathBuf,
}

#[derive(Debug)]
pub struct BuildReport {
    pub dir: PathBuf,
    pub exe: PathBuf,
    pub pak: PathBuf,
    pub files: usize,
    pub pak_bytes: u64,
}

pub fn build(project: &Project, opts: &BuildOptions, log: &mut dyn FnMut(String)) -> Result<BuildReport> {
    log(format!("Validating '{}'...", project.name()));
    let diagnostics = validate(project);
    for d in &diagnostics {
        log(format!("{}: {}", if d.severity == Severity::Error { "error" } else { "warning" }, d.message));
    }
    if has_errors(&diagnostics) {
        bail!("validation failed; fix the errors above and build again");
    }
    ensure!(opts.player.is_file(), "player executable not found at {}", opts.player.display());

    let mut pak = PakBuilder::default();
    pak.add(MANIFEST, toml::to_string_pretty(&project.manifest)?.into_bytes());

    for rel in project.scenes() {
        // Re-serialise compactly: smaller package, faster load, and proves it parses.
        let place = project.read_place(&rel)?;
        pak.add(&rel, serde_json::to_vec(&place)?);
    }
    // Everything else in the project, wherever it is kept: scripts, prefabs, meshes, textures, data.
    for rel in project.packaged_files() {
        if AssetKind::of(file_name(&rel)) != AssetKind::Scene {
            pak.add(&rel, fs::read(project.root.join(&rel))?);
        }
    }

    let dir = opts.out_dir.join(project.slug());
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let suffix = opts.player.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let exe = dir.join(format!("{}{suffix}", project.slug()));
    fs::copy(&opts.player, &exe).with_context(|| format!("copying player to {}", exe.display()))?;
    log(format!("Copied player -> {}", exe.display()));

    let pak_path = dir.join(PAK_NAME);
    let pak_bytes = pak.write(&pak_path)?;
    log(format!("Packed {} files -> {} ({} KiB)", pak.len(), pak_path.display(), pak_bytes / 1024));

    Ok(BuildReport { dir, exe, pak: pak_path, files: pak.len(), pak_bytes })
}

pub fn default_out_dir(project_root: &Path) -> PathBuf {
    project_root.join("build")
}
