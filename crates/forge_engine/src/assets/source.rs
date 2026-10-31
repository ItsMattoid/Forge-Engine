//! Where a running game reads its scenes and scripts from: a project folder
//! (editor / `--project` dev mode) or a packed `game.pak` (shipped builds).

use std::{fs, path::PathBuf, sync::Arc};

use anyhow::{anyhow, ensure, Result};

use super::pak::Pak;

pub const PAK_NAME: &str = "game.pak";

/// What [`GameSource::stamp`] returns: the file's size and (for a folder) its modification time.
pub type FileStamp = (u64, Option<std::time::SystemTime>);

#[derive(Clone)]
pub enum GameSource {
    Dir(PathBuf),
    Pak(Arc<Pak>),
}

/// Normalises a project-relative path and rejects anything that could leave the project:
/// absolute paths, `..`, and (on Windows) drive letters like `C:/x`, `C:x` and stream names
/// like `file:stream`, all of which `Path::join` would otherwise follow out of the project root.
pub fn clean_path(rel: &str) -> Result<String> {
    let s = rel.replace('\\', "/");
    let s = s.trim_start_matches("./").to_owned();
    ensure!(!s.starts_with('/'), "absolute paths are not allowed: {rel}");
    ensure!(!s.contains('\0'), "invalid path: {rel}");
    for part in s.split('/') {
        ensure!(part != "..", "path escapes the project: {rel}");
        ensure!(!part.contains(':'), "path must not contain ':': {rel}");
    }
    ensure!(
        std::path::Path::new(&s).components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)),
        "path must be relative to the project: {rel}"
    );
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_project_relative_paths() {
        assert_eq!(clean_path("scripts/a.luau").unwrap(), "scripts/a.luau");
        assert_eq!(clean_path(".\\assets\\m.obj").unwrap(), "assets/m.obj");
    }

    #[test]
    fn rejects_anything_that_leaves_the_project() {
        for bad in ["../x", "a/../../x", "/etc/passwd", "//server/share", "C:/Windows/win.ini", "C:\\Windows\\win.ini", "C:win.ini", "scripts/a.luau:stream"] {
            assert!(clean_path(bad).is_err(), "{bad} must be rejected");
        }
    }
}

impl GameSource {
    /// The scene file a script asked for with `game:LoadPlace(name)`. A path that ends in a scene
    /// extension (`Scenes/Level2.scene`, `scenes/level2.scene.json`) is used as it is; a bare name
    /// (`"Level2"`) is looked for as `Scenes/<name>.scene` and the older spellings, and failing that is
    /// returned as `Scenes/<name>.scene` so the error names a sensible file.
    pub fn resolve_scene(&self, name: &str) -> String {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".scene") || lower.ends_with(".scene.json") {
            return name.to_owned();
        }
        let candidates = [format!("Scenes/{name}.scene"), format!("Scenes/{name}.scene.json"), format!("scenes/{name}.scene"), format!("scenes/{name}.scene.json")];
        candidates.iter().find(|c| self.exists(c)).cloned().unwrap_or_else(|| candidates[0].clone())
    }

    pub fn read(&self, rel: &str) -> Result<Vec<u8>> {
        let rel = clean_path(rel)?;
        match self {
            Self::Dir(root) => {
                fs::read(root.join(&rel)).map_err(|e| anyhow!("reading '{rel}': {e}"))
            }
            Self::Pak(pak) => pak
                .get(&rel)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| anyhow!("'{rel}' is not in the game package")),
        }
    }

    pub fn read_string(&self, rel: &str) -> Result<String> {
        Ok(String::from_utf8(self.read(rel)?)?)
    }

    /// A cheap fingerprint of the file as it is now (one `stat`, no read): equal stamps mean the
    /// contents did not change, so something derived from them (compiled bytecode) is still good.
    /// `None` when the file cannot be found. A package never changes, so its stamp is just the size.
    pub fn stamp(&self, rel: &str) -> Option<FileStamp> {
        let rel = clean_path(rel).ok()?;
        match self {
            Self::Dir(root) => {
                let meta = fs::metadata(root.join(rel)).ok()?;
                meta.is_file().then(|| (meta.len(), meta.modified().ok()))
            }
            Self::Pak(pak) => pak.get(&rel).map(|bytes| (bytes.len() as u64, None)),
        }
    }

    /// Every file (project-relative, `/`-separated) whose path `wanted` accepts: a project folder is walked (not its hidden
    /// folders, nor `build`, `target` and `node_modules` at its top), a package is listed.
    pub fn list_files(&self, wanted: impl Fn(&str) -> bool) -> Vec<String> {
        let mut found: Vec<String> = match self {
            Self::Dir(root) => walkdir::WalkDir::new(root)
                .min_depth(1)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| !(e.file_type().is_dir() && (e.file_name().to_string_lossy().starts_with('.') || (e.depth() == 1 && matches!(e.file_name().to_string_lossy().as_ref(), "build" | "target" | "node_modules")))))
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .map(|e| e.path().strip_prefix(root).unwrap_or(e.path()).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
                .filter(|p| wanted(p))
                .collect(),
            Self::Pak(pak) => pak.paths().filter(|p| wanted(p)).map(str::to_owned).collect(),
        };
        found.sort();
        found
    }

    pub fn exists(&self, rel: &str) -> bool {
        let Ok(rel) = clean_path(rel) else { return false };
        match self {
            Self::Dir(root) => root.join(rel).is_file(),
            Self::Pak(pak) => pak.get(&rel).is_some(),
        }
    }
}
