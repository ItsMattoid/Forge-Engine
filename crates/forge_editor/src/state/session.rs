//! Running the game in the player, validating the project and building it.

use std::{
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    sync::mpsc::Sender,
};

use forge_engine::project::{
    export::{self, BuildOptions},
    launcher,
};

use super::{Editor, Level};
use crate::dock::Panel;

fn spawn_reader(r: impl Read + Send + 'static, tx: Sender<String>) {
    std::thread::spawn(move || {
        for line in BufReader::new(r).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
}

impl Editor {
    /// Runs the game in its own window (a separate player process). The scene is saved first,
    /// because the player reads it from disk. (The in-viewport Play is `start_play`.)
    pub fn play_in_window(&mut self) {
        self.stop_play();
        if self.child.is_some() {
            self.stop();
        }
        let Some(root) = self.project.as_ref().map(|p| p.root.clone()) else {
            return self.warn("Open a project first.");
        };
        self.dirty = true; // always write what is on screen before playing
        self.save_all();
        match launcher::spawn_player(&root, true) {
            Ok(mut child) => {
                if let Some(o) = child.stdout.take() {
                    spawn_reader(o, self.tx.clone());
                }
                if let Some(e) = child.stderr.take() {
                    spawn_reader(e, self.tx.clone());
                }
                self.info("Game started.");
                self.child = Some(child);
                self.show_panel(Panel::Output);
            }
            Err(e) => self.error(format!("{e:#}")),
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
            self.info("Game stopped.");
        }
    }

    pub fn validate(&mut self) {
        let Some(project) = &self.project else { return };
        let diagnostics = export::validate(project);
        let count = diagnostics.len();
        for d in diagnostics {
            let level = if d.severity == export::Severity::Error { Level::Error } else { Level::Warn };
            self.push_log(level, d.message);
        }
        if count == 0 {
            self.info("Validation passed: no problems found.");
        }
        self.show_panel(Panel::Output);
    }

    pub fn build(&mut self, out_dir: PathBuf) {
        self.dirty = true;
        self.save_all();
        let Some(project) = self.project.clone() else { return };
        let player = match launcher::player_path() {
            Ok(p) => p,
            Err(e) => return self.error(format!("{e:#}")),
        };
        let mut lines = Vec::new();
        let result = export::build(&project, &BuildOptions { out_dir, player }, &mut |l| lines.push(l));
        for l in lines {
            self.info(l);
        }
        match result {
            Ok(r) => self.info(format!("Build complete: {}", r.dir.display())),
            Err(e) => self.error(format!("Build failed: {e:#}")),
        }
        self.show_panel(Panel::Output);
    }
}
