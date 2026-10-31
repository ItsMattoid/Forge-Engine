//! Play inside the editor: the game runs in the viewport, on the very data model the editor edits.
//!
//! Starting a session takes a snapshot of the scene, attaches a script host to the live data
//! model and lets scripts and physics run. Everything the editor does while it runs (the Explorer,
//! the Properties panel, inserting and deleting, moving things with the gizmo) changes the running
//! game immediately. Stopping throws the host away and puts the snapshot back, so nothing the game
//! or the live edits did is left in the scene (the same as Roblox Studio's Play).

use bevy::math::Vec2;
use forge_engine::{
    dm::{file::PlaceFile, D_V, D_X},
    scripting::{FrameInput, LogLevel, ScriptHost},
};

use super::{Editor, Level};
use crate::dock::Panel;

/// A running game session.
pub struct PlaySession {
    pub host: ScriptHost,
    /// The scene as it was when Play was pressed; restored on Stop.
    snapshot: PlaceFile,
    /// Whether the scene had unsaved edits before Play (edits made while playing are not kept).
    dirty_before: bool,
    /// The scene's instances in file order before Play: Stop rebuilds them, and the undo history
    /// is pointed at the rebuilt ones by position.
    order_before: Vec<forge_engine::dm::Id>,
    /// What was selected before Play (selecting while playing is not kept).
    selection_before: super::SelectionState,
    /// Look through the game's own camera (otherwise the editor's free camera stays in control).
    pub game_camera: bool,
    /// The viewport has the keyboard and mouse: input goes to the game, not to the editor.
    pub focused: bool,
}

impl PlaySession {
    /// Whether keys and the mouse are being sent to the game right now.
    pub fn takes_input(&self) -> bool {
        self.game_camera && self.focused
    }
}

impl Editor {
    pub fn playing(&self) -> bool {
        self.play.is_some()
    }

    /// The game has the keyboard and mouse (so the editor must not act on them).
    pub fn game_has_input(&self) -> bool {
        self.play.as_ref().is_some_and(PlaySession::takes_input)
    }

    /// Whether the viewport's Game tab is the one in front: a game is playing and the viewport looks through its
    /// camera. (Otherwise the Editor tab is: the editor camera, the tools and the editing visuals.)
    pub fn game_view_active(&self) -> bool {
        self.play.as_ref().is_some_and(|p| p.game_camera)
    }

    /// Brings the Game or the Editor tab of the viewport to the front. Only the camera and who gets the keyboard and
    /// mouse change: the game keeps running and the scene and the editor camera are left as they are. Does nothing
    /// without a running game (there is no Game tab then).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_game_view(&mut self, game: bool) {
        let Some(play) = self.play.as_mut() else { return };
        if play.game_camera != game {
            play.game_camera = game;
            play.focused = game;
        }
        self.show_panel(if game { Panel::Game } else { Panel::Viewport });
    }

    /// The tabs are the truth about which view is in front: whichever the user clicked (or set_game_view chose)
    /// decides, once per frame after the dock was drawn. Moving to the Game tab gives the game the keyboard and
    /// mouse, as clicking into the Game view always did; going to the Editor tab gives them back.
    pub fn sync_game_view_from_dock(&mut self) {
        let game = self.dock.game_in_front();
        if let Some(play) = self.play.as_mut().filter(|p| p.game_camera != game) {
            play.game_camera = game;
            play.focused = game;
        }
    }

    pub fn toggle_play(&mut self) {
        if self.playing() {
            self.stop_play();
        } else {
            self.start_play();
        }
    }

    pub fn start_play(&mut self) {
        if self.play.is_some() {
            return;
        }
        if self.project.is_none() {
            return self.warn("Open a project first.");
        }
        // Scripts are read from disk: make sure the one being edited is what runs.
        self.save_open_script();
        // Edits made so far are a finished undo step; edits made while playing are not recorded.
        self.flush_edits();
        let snapshot = self.snapshot();
        let order_before = self.dm.borrow().persisted_order();
        let selection_before = self.selection_state();
        let host = match ScriptHost::new(self.source.clone(), self.dm.clone()) {
            Ok(h) => h,
            Err(e) => return self.error(format!("could not start the game: {e:#}")),
        };
        host.capture_logs(true);
        {
            let mut dm = self.dm.borrow_mut();
            Self::forget_runtime_state(&mut dm);
            dm.stop_journal();
            // The renderer draws a playing game differently (hidden parts stay hidden).
            dm.mark_all(D_X | D_V);
        }
        host.start_all();
        self.play = Some(PlaySession { host, snapshot, dirty_before: self.dirty, order_before, selection_before, game_camera: true, focused: true });
        self.forget_duplicate();
        // The viewport gets a second tab, the Game, in front. (The Editor tab is the viewport as it was: its camera
        // and the scene are untouched.)
        self.show_panel(Panel::Viewport);
        self.show_panel(Panel::Game);
        self.show_panel(Panel::Output);
        self.info("Playing in the viewport. Edit anything live; Stop (Shift+F5) puts the scene back as it was.");
    }

    pub fn stop_play(&mut self) {
        let Some(session) = self.play.take() else { return };
        let PlaySession { host, snapshot, dirty_before, order_before, selection_before, .. } = session;
        self.forget_duplicate();
        // The viewport is one tab again.
        self.hide_panel(Panel::Game);
        let tail = host.drain_logs();
        self.show_game_log(tail);
        drop(host);

        {
            let mut dm = self.dm.borrow_mut();
            dm.reset_runtime_props();
            let _ = dm.load_place(&snapshot);
            Self::forget_runtime_state(&mut dm);
            dm.mark_all(D_X | D_V);
            // The scene was rebuilt from the snapshot, so every instance is a new one; the undo
            // history keeps working by following each old instance to its replacement.
            self.history.remap(&order_before, &dm.persisted_order());
            dm.start_journal();
        }
        // The selection that was there before Play comes back (the objects were rebuilt: follow them to their replacements).
        let restored = self.history.resolve_state(&selection_before);
        self.apply_selection_state(&restored);
        self.sel_committed = self.selection_state();
        self.dirty = dirty_before;
        self.info("Stopped. The scene is back to how it was before Play.");
    }

    /// Drops everything a script host leaves in the data model (listeners, queued events, tags).
    fn forget_runtime_state(dm: &mut forge_engine::dm::DataModel) {
        dm.watched.clear();
        dm.touch_watch.clear();
        dm.events.clear();
        dm.pending_scripts.clear();
        dm.dead_scripts.clear();
        dm.dead_watched.clear();
        dm.tags.clear();
    }

    fn show_game_log(&mut self, lines: Vec<forge_engine::scripting::LogLine>) {
        for l in lines {
            let level = match l.level {
                LogLevel::Info => Level::Info,
                LogLevel::Warn => Level::Warn,
                LogLevel::Error => Level::Error,
            };
            self.push_log(level, format!("[game] {}", l.text));
        }
    }

    /// Advances the running game by one frame. `input` is what the viewport hands to the game.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn tick_play(&mut self, input: &FrameInput) {
        self.tick_play_with(input, None);
    }

    /// [`tick_play`] with the physics world the game's physics steps in, so scripts can use it this frame (impulses,
    /// ray casts, overlap tests).
    pub fn tick_play_with(&mut self, input: &FrameInput, world: Option<&forge_engine::physics::PhysicsWorld>) {
        let (quit, place, logs) = {
            let Some(play) = self.play.as_mut() else {
                // Not playing: nothing may pile up in the data model's script queues.
                let mut dm = self.dm.borrow_mut();
                dm.pending_scripts.clear();
                dm.dead_scripts.clear();
                dm.dead_watched.clear();
                return;
            };
            play.host.frame_with(input, world);
            let requests = play.host.take_requests();
            (requests.quit, requests.place, play.host.drain_logs())
        };
        self.show_game_log(logs);

        if let Some(scene) = place {
            self.play_load_scene(&scene);
        }
        if quit {
            self.stop_play();
        }
    }

    /// A script asked for another scene (`game:LoadPlace`). It replaces the running scene; Stop
    /// still returns to the one that was open in the editor.
    fn play_load_scene(&mut self, scene: &str) {
        let scene = &self.source.resolve_scene(scene);
        let place = self.project.as_ref().map(|p| p.read_place(scene));
        let Some(play) = self.play.as_mut() else { return };
        match place {
            Some(Ok(place)) => {
                if let Err(e) = play.host.load_place(&place) {
                    self.error(format!("could not load scene '{scene}': {e:#}"));
                }
            }
            Some(Err(e)) => self.error(format!("could not load scene '{scene}': {e:#}")),
            None => {}
        }
    }

    /// The mouse position inside the viewport, as scripts see it.
    pub fn viewport_cursor(pointer: Option<Vec2>, min: Vec2, max: Vec2) -> Option<Vec2> {
        let p = pointer?;
        (p.x >= min.x && p.y >= min.y && p.x <= max.x && p.y <= max.y).then(|| p - min)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy::input::{keyboard::KeyCode, mouse::MouseButton, ButtonInput};
    use forge_engine::{core::Value, project::Project};

    use super::*;

    /// A generated starter project in a temp folder, removed afterwards.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_play_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Play Test").expect("create project");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn frames(ed: &mut Editor, n: usize) {
        let (keys, mouse) = (ButtonInput::<KeyCode>::default(), ButtonInput::<MouseButton>::default());
        for _ in 0..n {
            ed.tick_play(&FrameInput { dt: 1.0 / 60.0, keys: &keys, mouse: &mouse, mouse_delta: Vec2::ZERO, cursor: None, wheel: 0.0 });
        }
    }

    #[test]
    fn play_runs_the_game_on_the_live_scene_and_stop_restores_it() {
        let scratch = Scratch::new();
        let mut ed = Editor::new(Some(scratch.0.clone()));
        assert!(ed.project.is_some(), "the project opened");

        let (spinner, coins) = {
            let dm = ed.dm.borrow();
            (dm.find_first_child(dm.workspace, "Spinner", false).expect("spinner"), dm.find_first_child(dm.service("ReplicatedStorage").unwrap(), "Coins", false).expect("coins value"))
        };
        let before = ed.dm.borrow().vec3(spinner, "Orientation");
        ed.selected = Some(spinner);
        ed.dirty = false;

        ed.start_play();
        assert!(ed.playing(), "Play started: {:?}", ed.log.iter().map(|l| l.text.as_str()).collect::<Vec<_>>());
        frames(&mut ed, 30);
        let during = ed.dm.borrow().vec3(spinner, "Orientation");
        assert_ne!(during, before, "the Spin script rotates the part while playing");

        // Live editing: a change made through the editor reaches the running scripts.
        ed.dm.borrow_mut().set_prop(coins, "Value", Value::Num(3.0)).unwrap();
        frames(&mut ed, 3);
        assert!(ed.log.iter().any(|l| l.text.starts_with("[game] Coins:") && l.text.ends_with('3')), "the GameManager script saw the edit; log: {:?}", ed.log.iter().map(|l| l.text.as_str()).collect::<Vec<_>>());

        // While playing nothing is written over the scene on disk.
        let on_disk = std::fs::read_to_string(scratch.0.join("Scenes/MainScene.scene")).unwrap();
        ed.dirty = true;
        ed.save_all();
        assert_eq!(std::fs::read_to_string(scratch.0.join("Scenes/MainScene.scene")).unwrap(), on_disk, "saving during Play must not write the running game");

        ed.stop_play();
        assert!(!ed.playing());
        // Stop reloads the scene, so the instances are new ones: look them up again.
        let (spinner, coins) = {
            let dm = ed.dm.borrow();
            (dm.find_first_child(dm.workspace, "Spinner", false).expect("spinner after Stop"), dm.find_first_child(dm.service("ReplicatedStorage").unwrap(), "Coins", false).expect("coins after Stop"))
        };
        assert_eq!(ed.dm.borrow().vec3(spinner, "Orientation"), before, "Stop puts the scene back");
        assert_eq!(ed.dm.borrow().num(coins, "Value", -1.0), 0.0, "live edits are discarded");
        assert!(!ed.dirty, "the scene is as clean as it was before Play");
        assert_eq!(ed.selected, Some(spinner), "the selection survives");
        let uis = ed.dm.borrow().service("UserInputService").unwrap();
        assert_eq!(ed.dm.borrow().string(uis, "MouseBehavior"), "Default", "the game's mouse capture is released");
    }

    #[test]
    fn the_game_and_editor_views_switch_without_touching_the_game_the_scene_or_the_editor() {
        let scratch = Scratch::new();
        let mut ed = Editor::new(Some(scratch.0.clone()));

        // No game, no Game View: asking for it does nothing, and the Editor View is the one shown.
        ed.set_game_view(true);
        assert!(!ed.playing() && !ed.game_view_active());

        // Play opens in the Game View, with the keyboard and mouse going to the game.
        ed.start_play();
        assert!(ed.playing() && ed.game_view_active() && ed.game_has_input());
        frames(&mut ed, 10);
        let spinner = {
            let dm = ed.dm.borrow();
            dm.find_first_child(dm.workspace, "Spinner", false).expect("spinner")
        };
        let turned = ed.dm.borrow().vec3(spinner, "Orientation");

        // To the Editor View: the game goes on (the same scene, not restarted) and the editor has the keyboard.
        ed.set_game_view(false);
        assert!(ed.playing() && !ed.game_view_active() && !ed.game_has_input());
        frames(&mut ed, 10);
        let later = ed.dm.borrow().vec3(spinner, "Orientation");
        assert_ne!(later, turned, "the game kept running while the Editor View was in front");
        assert!(ed.dm.borrow().exists(spinner), "the scene was not rebuilt");

        // And back: the game has the input again.
        ed.set_game_view(true);
        assert!(ed.game_view_active() && ed.game_has_input());
        ed.set_game_view(true);
        assert!(ed.game_view_active(), "choosing the open tab again changes nothing");

        // Stop goes back to the Editor View; the next Play opens the Game View again.
        ed.set_game_view(false);
        ed.stop_play();
        assert!(!ed.playing() && !ed.game_view_active());
        ed.start_play();
        assert!(ed.game_view_active(), "a new session starts in the Game View whatever the last one ended on");
    }

    #[test]
    fn play_twice_in_a_row_works() {
        let scratch = Scratch::new();
        let mut ed = Editor::new(Some(scratch.0.clone()));
        for _ in 0..2 {
            ed.start_play();
            assert!(ed.playing());
            frames(&mut ed, 5);
            ed.stop_play();
            assert!(!ed.playing());
        }
        let walls = ed.dm.borrow().find_first_child(ed.dm.borrow().workspace, "Wall", false);
        assert!(walls.is_some(), "the scene is intact after two sessions");
    }
}
