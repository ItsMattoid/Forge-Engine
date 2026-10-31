//! Locating and launching the sibling binaries (`forge_player`, `forge_editor`).
//! All Forge binaries are expected to live in the same directory.

use std::{
    env,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use anyhow::{anyhow, Context, Result};

fn exe_name(base: &str) -> String {
    format!("{base}{}", env::consts::EXE_SUFFIX)
}

fn sibling(base: &str) -> Result<PathBuf> {
    let dir = env::current_exe()?.parent().map(Path::to_path_buf).ok_or_else(|| anyhow!("cannot determine the executable directory"))?;
    let path = dir.join(exe_name(base));
    if path.is_file() {
        Ok(path)
    } else {
        Err(anyhow!("'{}' was not found. Build the whole workspace first (`cargo build --release`).", path.display()))
    }
}

pub fn player_path() -> Result<PathBuf> {
    sibling("forge_player")
}

pub fn editor_path() -> Result<PathBuf> {
    sibling("forge_editor")
}

/// Starts the player on a project folder (dev mode: reads files directly, no packaging).
/// `capture` pipes stdout/stderr so the caller can show the game's log.
pub fn spawn_player(project_root: &Path, capture: bool) -> Result<Child> {
    let player = player_path()?;
    let mut cmd = Command::new(&player);
    cmd.arg("--project").arg(project_root);
    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).env("NO_COLOR", "1");
    }
    cmd.spawn().with_context(|| format!("starting {}", player.display()))
}
