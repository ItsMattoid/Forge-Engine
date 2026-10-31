//! A small harness for tests that run real Luau against a real data model: [`Game`] runs a script as the
//! only script of a fresh game, steps frames (with the physics world, as the apps do) and hands back what the
//! script reported.
//!
//! A script reports through `Out` (a StringValue in ReplicatedStorage) or by printing: [`Game::logs`]. [`checks`] wraps
//! the common case: a script made of `check(...)` / `equal(...)` lines that must all hold.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use bevy::{
    input::{keyboard::KeyCode, mouse::MouseButton, ButtonInput},
    math::Vec2,
};

use super::*;
use crate::{
    core::Value as DV,
    physics::{physics_config, Box3dWorld, SUB_STEPS},
};

pub struct Game {
    pub dm: Rc<RefCell<DataModel>>,
    pub host: ScriptHost,
    physics: Box3dWorld,
    dir: std::path::PathBuf,
    keys: ButtonInput<KeyCode>,
    mouse: ButtonInput<MouseButton>,
    cursor: Option<Vec2>,
}

impl Drop for Game {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

impl Game {
    /// A game whose files are `files` (path -> source), with an empty data model besides the services.
    pub fn with_files(files: &[(&str, &str)]) -> Game {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("forge_compat_{}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, src) in files {
            if let Some(parent) = dir.join(name).parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(dir.join(name), src).unwrap();
        }
        let dm = Rc::new(RefCell::new(DataModel::new()));
        let host = ScriptHost::new(GameSource::Dir(dir.clone()), dm.clone()).expect("host");
        host.capture_logs(true);
        {
            let mut d = dm.borrow_mut();
            let out = d.create("StringValue").unwrap();
            d.set_name(out, "Out");
            let rs = d.service("ReplicatedStorage").unwrap();
            d.set_parent(out, Some(rs)).unwrap();
        }
        Game { dm, host, physics: Box3dWorld::new(physics_config()), dir, keys: ButtonInput::default(), mouse: ButtonInput::default(), cursor: None }
    }

    /// A game running `source` as its only script (in ServerScriptService); the first frame has run.
    pub fn new(source: &str) -> Game {
        let mut g = Game::with_files(&[("main.luau", source)]);
        g.add_script("Main", "ServerScriptService", "main.luau", "Script");
        g.host.start_all();
        g.frames(1);
        g
    }

    /// Adds a script-like instance (`class` is Script, LocalScript or ModuleScript) running `file`.
    pub fn add_script(&mut self, name: &str, parent: &str, file: &str, class: &str) {
        let mut d = self.dm.borrow_mut();
        let s = d.create(class).unwrap();
        d.set_name(s, name);
        d.set_prop(s, "SourceFile", DV::Str(file.into())).unwrap();
        let p = d.service(parent).unwrap();
        d.set_parent(s, Some(p)).unwrap();
    }

    /// Runs `n` frames the way the apps do: the physics step (`pre_step`, the world, `post_step`), then the frame of scripts
    /// with the world in reach. Keys and buttons pressed or released since the last frame are seen by this one only.
    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            let world = self.physics.world();
            self.host.physics_pre_step(world, 1.0 / 60.0);
            world.step(1.0 / 60.0, SUB_STEPS);
            self.host.physics_post_step(world, 0.0);
            let input = FrameInput { dt: 1.0 / 60.0, keys: &self.keys, mouse: &self.mouse, mouse_delta: Vec2::ZERO, cursor: self.cursor, wheel: 0.0 };
            self.host.frame_with(&input, Some(world));
            self.keys.clear();
            self.mouse.clear();
        }
    }

    /// Presses a key (the next frame sees it begin and, until `release_key`, held).
    pub fn press_key(&mut self, key: KeyCode) {
        self.keys.press(key);
    }

    pub fn release_key(&mut self, key: KeyCode) {
        self.keys.release(key);
    }

    pub fn press_mouse(&mut self, button: MouseButton) {
        self.mouse.press(button);
    }

    pub fn release_mouse(&mut self, button: MouseButton) {
        self.mouse.release(button);
    }

    /// Puts the mouse at a pixel of the window.
    pub fn move_cursor(&mut self, at: Vec2) {
        self.cursor = Some(at);
    }

    /// What the script wrote to `Out`.
    pub fn out(&self) -> String {
        let d = self.dm.borrow();
        let rs = d.service("ReplicatedStorage").unwrap();
        let out = d.find_child(rs, "Out").unwrap();
        d.string(out, "Value")
    }

    /// Everything printed, warned or raised since the last call, one per line.
    pub fn logs(&self) -> String {
        self.host.drain_logs().iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n")
    }
}

/// The helpers a checking script starts with: `check(name, condition)` and `equal(name, got, expected)` note what fails,
/// `finish()` reports it.
pub const CHECKS: &str = r#"
local failures = {}
local function check(name, ok, detail)
	if not ok then
		table.insert(failures, name .. (detail ~= nil and (": " .. tostring(detail)) or ""))
	end
end
local function equal(name, got, expected)
	if got ~= expected then
		table.insert(failures, name .. ": expected " .. tostring(expected) .. ", got " .. tostring(got))
	end
end
local function throws(name, f, fragment)
	local ok, err = pcall(f)
	if ok then
		table.insert(failures, name .. ": expected an error")
	elseif fragment and not string.find(tostring(err), fragment, 1, true) then
		table.insert(failures, name .. ": error '" .. tostring(err) .. "' does not mention '" .. fragment .. "'")
	end
end
local function near(a, b, eps)
	return math.abs(a - b) <= (eps or 1e-4)
end
local function finish()
	game:GetService("ReplicatedStorage").Out.Value = #failures == 0 and "ok" or table.concat(failures, "; ")
end
"#;

/// Runs a script made of checks (see [`CHECKS`]) for `frames` frames and asserts that every check held.
pub fn checks_for(body: &str, frames: usize) {
    let mut g = Game::new(&format!("{CHECKS}\n{body}\nfinish()"));
    g.frames(frames);
    let (out, logs) = (g.out(), g.logs());
    assert_eq!(out, "ok", "some checks failed:\n{out}\n--- output:\n{logs}");
}

/// [`checks_for`] a few frames (enough for a script that waits a frame or two).
pub fn checks(body: &str) {
    checks_for(body, 4);
}
