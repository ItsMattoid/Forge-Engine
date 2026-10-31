//! Luau scripting host.
//!
//! * One sandboxed Luau VM (`lua.sandbox(true)`), compiled at optimisation level 2.
//! * Scripts are instances (`Script`, `LocalScript`, `ModuleScript`) in the data
//!   model; each runs in its own coroutine with its own environment (`script`).
//! * Frame order: input events -> `Stepped` -> `Heartbeat` -> `task.wait` wake-ups ->
//!   `RenderStepped`. Instance events (`Changed`, `Touched`, ...) are dispatched after each
//!   phase. Physics is not a phase of the frame: it steps at a fixed rate in Bevy's `FixedUpdate`, just before
//!   the frame (see [`ScriptHost::physics_pre_step`]), so what the step produced (`Touched`, new positions)
//!   is what the frame's scripts see, and what the scripts change is applied before the next step.
//! * A wall-clock budget per phase of the frame interrupts runaway scripts; script errors are
//!   logged and only end the failing thread.
//! * A script's connections and threads end with it: when the Script is destroyed, disabled or
//!   moved out of the places scripts run in, everything it started is stopped.

mod api;
mod keys;
mod lua_types;
mod profile;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::Result;
use bevy::{
    input::{keyboard::KeyCode, mouse::MouseButton, ButtonInput},
    log::{error, info, warn},
    math::Vec2,
};
use mlua::{Compiler, Function, Lua, Result as LuaResult, Table, Thread, ThreadStatus, Value, VmState};

pub use api::{Ctx, InstanceRef, Requests};
pub use profile::{ScriptCost, ScriptProfile, PHASES};
use profile::{Lap, P_EVENTS, P_HEARTBEAT, P_INPUT, P_PHYSICS, P_RENDER_STEPPED, P_START, P_STEPPED, P_TIMERS};

use crate::{
    assets::GameSource,
    core::{registry, FastMap, Id},
    dm::{file::PlaceFile, DataModel, Event},
    physics::{PhysicsBridge, WorldScope},
};

#[cfg(test)]
mod compat_tests;
#[cfg(test)]
mod physics_tests;
#[cfg(test)]
mod testkit;

const PRELUDE: &str = include_str!("prelude.luau");
/// The Roblox value types that are only data (`UDim2`, `NumberSequence`, `BrickColor` ...), loaded right after the prelude.
const DATATYPES: &str = include_str!("datatypes.luau");

/// mlua's error type is not `Send + Sync` (we run without its `send` feature), so it
/// cannot convert into `anyhow::Error` with `?`. This adapter keeps the message.
trait LuaCtx<T> {
    fn lua(self) -> Result<T>;
}

impl<T> LuaCtx<T> for mlua::Result<T> {
    fn lua(self) -> Result<T> {
        self.map_err(|e| anyhow::anyhow!("{e}"))
    }
}

/// Everything the host needs to know about the current frame.
pub struct FrameInput<'a> {
    pub dt: f32,
    pub keys: &'a ButtonInput<KeyCode>,
    pub mouse: &'a ButtonInput<MouseButton>,
    pub mouse_delta: Vec2,
    pub cursor: Option<Vec2>,
    pub wheel: f32,
}

struct Hot {
    heartbeat: Table,
    stepped: Table,
    render_stepped: Table,
    /// RunService's newer per-frame signals: each fires with the frame it is named for.
    pre_render: Table,
    pre_animation: Table,
    pre_simulation: Table,
    post_simulation: Table,
    began: Table,
    ended: Table,
    changed: Table,
}

impl Hot {
    fn new(ctx: &Rc<Ctx>) -> LuaResult<Self> {
        let (run, uis) = {
            let dm = ctx.dm.borrow();
            (dm.service("RunService"), dm.service("UserInputService"))
        };
        let (Some(run), Some(uis)) = (run, uis) else { return Err(mlua::Error::runtime("missing services")) };
        Ok(Hot {
            heartbeat: ctx.signal(run, "Heartbeat", true, false)?,
            stepped: ctx.signal(run, "Stepped", true, false)?,
            render_stepped: ctx.signal(run, "RenderStepped", true, false)?,
            pre_render: ctx.signal(run, "PreRender", true, false)?,
            pre_animation: ctx.signal(run, "PreAnimation", true, false)?,
            pre_simulation: ctx.signal(run, "PreSimulation", true, false)?,
            post_simulation: ctx.signal(run, "PostSimulation", true, false)?,
            began: ctx.signal(uis, "InputBegan", false, false)?,
            ended: ctx.signal(uis, "InputEnded", false, false)?,
            changed: ctx.signal(uis, "InputChanged", false, false)?,
        })
    }
}

/// How long one phase of a frame (input, physics, Heartbeat ...) may spend in scripts
/// before the running script is interrupted. Each phase gets a fresh budget, so a slow
/// phase cannot starve the ones after it.
const PHASE_BUDGET: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// A line a script printed (or an error it raised), kept for hosts that show a console.
#[derive(Clone, Debug)]
pub struct LogLine {
    pub level: LogLevel,
    pub text: String,
}

/// Where script output goes besides the process log: a buffer the embedding application can drain.
#[derive(Clone, Default)]
struct LogSink {
    capture: Rc<Cell<bool>>,
    lines: Rc<RefCell<Vec<LogLine>>>,
    /// Lines not yet delivered to `LogService.MessageOut` (the host empties it with the frame's events).
    feed: Rc<RefCell<Vec<(LogLevel, String)>>>,
}

/// How many lines wait for `LogService` at most (a script printing in a tight loop must not grow this without end).
const MAX_FEED: usize = 2_000;

impl LogSink {
    fn emit(&self, level: LogLevel, text: String) {
        match level {
            LogLevel::Info => info!(target: "script", "{text}"),
            LogLevel::Warn => warn!(target: "script", "{text}"),
            LogLevel::Error => error!(target: "script", "{text}"),
        }
        {
            let mut feed = self.feed.borrow_mut();
            if feed.len() < MAX_FEED {
                feed.push((level, text.clone()));
            }
        }
        if self.capture.get() {
            let mut lines = self.lines.borrow_mut();
            lines.push(LogLine { level, text });
            if lines.len() > 10_000 {
                lines.drain(..2_000);
            }
        }
    }
}

/// The prelude's profiling functions (see `profile`).
struct ProfilerFns {
    set: Function,
    adopt: Function,
    charge_thread: Function,
    charge_script: Function,
    take: Function,
}

pub struct ScriptHost {
    lua: Lua,
    pub ctx: Rc<Ctx>,
    mkinput: Function,
    reap: Function,
    reset_modules: Function,
    hot: Hot,
    deadline: Rc<Cell<Instant>>,
    budget: Duration,
    log: LogSink,
    profiler: ProfilerFns,
    /// Whether the services were told that the game started (`ClassBehavior::on_started`).
    started: Cell<bool>,
    /// Whether per-script costs are being measured.
    profiling: Cell<bool>,
    profile: RefCell<ScriptProfile>,
    /// What the physics steps since the last frame took (they run outside it, in `FixedUpdate`), in milliseconds.
    physics_ms: Cell<f32>,
}

/// How scripts are compiled: the VM's default compiler and the one that caches bytecode (see `Ctx::chunks`).
fn script_compiler() -> Compiler {
    Compiler::new().set_optimization_level(2).set_debug_level(1)
}

fn str_seq(lua: &Lua, items: impl IntoIterator<Item = &'static str>) -> LuaResult<Table> {
    lua.create_sequence_from(items)
}

/// One enum as the prelude builds it from: `{ name, items = { names }, numbers = { values }, aliases = { { alias, item } } }`.
fn enum_def(lua: &Lua, name: &str, items: &[&'static str], numbers: &[i32], aliases: &[(&'static str, &'static str)]) -> LuaResult<Table> {
    let def = lua.create_table()?;
    def.set("name", name)?;
    def.set("items", str_seq(lua, items.iter().copied())?)?;
    def.set("numbers", lua.create_sequence_from(numbers.iter().copied())?)?;
    let pairs = lua.create_table()?;
    for (alias, item) in aliases {
        pairs.push(lua.create_sequence_from([*alias, *item])?)?;
    }
    def.set("aliases", pairs)?;
    Ok(def)
}

/// Exposes the registry's enums to the prelude as `__enums`.
fn install_enums(lua: &Lua) -> Result<()> {
    let defs = lua.create_table().lua()?;
    for e in registry().enums() {
        let numbers: Vec<i32> = (0..e.values.len()).map(|i| e.number(i)).collect();
        defs.push(enum_def(lua, e.name, e.values, &numbers, e.aliases).lua()?).lua()?;
    }
    lua.globals().set("__enums", defs).lua()
}

/// Gives the datatypes library the `BrickColor` palette as `__brickcolors`: `{ number, name, r, g, b }` (0..255).
fn install_brickcolors(lua: &Lua) -> Result<()> {
    let palette = lua.create_table().lua()?;
    for c in crate::core::brickcolor::PALETTE {
        let entry = lua.create_table().lua()?;
        entry.push(c.number).lua()?;
        entry.push(c.name).lua()?;
        for channel in c.rgb {
            entry.push(channel).lua()?;
        }
        palette.push(entry).lua()?;
    }
    lua.globals().set("__brickcolors", palette).lua()
}

/// Classes may ship Luau code for methods that are easiest to write in Luau (they
/// yield, or build on signals). Each snippet sees `methods` (this class's method
/// table), `Signal`, `task`, `Enum` and `define(name, value)` for globals.
fn load_class_snippets(lua: &Lua, snippet_env: &Function, class_methods: &Table) -> Result<()> {
    for class in registry().classes() {
        if class.desc.luau.is_empty() {
            continue;
        }
        let table: Table = match class_methods.get::<Option<Table>>(class.name).lua()? {
            Some(t) => t,
            None => {
                let t = lua.create_table().lua()?;
                class_methods.set(class.name, t.clone()).lua()?;
                t
            }
        };
        let env: Table = snippet_env.call(table).lua()?;
        let chunk = lua.load(class.desc.luau).set_name(format!("={}.luau", class.name)).set_environment(env).into_function().lua()?;
        chunk.call::<()>(()).lua()?;
    }
    Ok(())
}

/// Stops scripts that run for too long (infinite loops). The host re-arms the
/// returned deadline before running any script code.
fn install_time_budget(lua: &Lua, budget: Duration) -> Rc<Cell<Instant>> {
    let deadline = Rc::new(Cell::new(Instant::now() + budget));
    // Checking the clock on every interrupt would be expensive; sample it.
    let (d, ticks) = (deadline.clone(), Cell::new(0u32));
    lua.set_interrupt(move |_| {
        let n = ticks.get().wrapping_add(1);
        ticks.set(n);
        if n & 0x3FF == 0 && Instant::now() > d.get() {
            return Err(mlua::Error::runtime("script exceeded its time budget (infinite loop?)"));
        }
        Ok(VmState::Continue)
    });
    deadline
}

impl Drop for ScriptHost {
    fn drop(&mut self) {
        // The data model outlives the host (the editor plays on its own): it stops recording for a simulation that is gone.
        if let Ok(mut dm) = self.ctx.dm.try_borrow_mut() {
            dm.phys.detach();
        }
    }
}

impl ScriptHost {
    pub fn new(source: GameSource, data_model: Rc<RefCell<DataModel>>) -> Result<Self> {
        let lua = Lua::new();
        lua.set_compiler(script_compiler());
        let globals = lua.globals();

        // Phase A: things the prelude needs.
        lua_types::install(&lua).lua()?;
        let log = LogSink::default();
        let sink = log.clone();
        globals
            .set(
                "__log",
                lua.create_function(move |_, (level, msg): (i32, String)| {
                    let level = match level {
                        0 => LogLevel::Info,
                        1 => LogLevel::Warn,
                        _ => LogLevel::Error,
                    };
                    sink.emit(level, msg);
                    Ok(())
                })
                .lua()?,
            )
            .lua()?;

        install_enums(&lua)?;
        install_brickcolors(&lua)?;

        let methods = lua.create_table().lua()?;
        let class_methods = lua.create_table().lua()?;

        let exports: Table = lua.load(PRELUDE).set_name("=prelude").into_function().lua()?.call(()).lua()?;
        let datatypes: Table = lua.load(DATATYPES).set_name("=datatypes").into_function().lua()?.call(()).lua()?;

        let snippet_env: Function = exports.get("snippet_env").lua()?;
        load_class_snippets(&lua, &snippet_env, &class_methods)?;

        // Phase B: things that need the context.
        let cache: Table = lua.load("return setmetatable({}, { __mode = 'v' })").eval().lua()?;
        let ctx = Rc::new(Ctx {
            dm: data_model,
            physics: RefCell::new(PhysicsBridge::new()),
            source,
            cache,
            methods,
            class_methods,
            enums: exports.get("enums").lua()?,
            signal_new: exports.get("signal_new").lua()?,
            signal_fire: exports.get("signal_fire").lua()?,
            new_env: exports.get("new_env").lua()?,
            signals: RefCell::new(Default::default()),
            method_cache: RefCell::new(Default::default()),
            input: RefCell::new(Default::default()),
            time: Cell::new(0.0),
            dt: Cell::new(0.0),
            requests: RefCell::new(Requests::default()),
            sched: RefCell::new(Default::default()),
            data_encode: datatypes.get("encode").lua()?,
            data_decode: datatypes.get("decode").lua()?,
            data_tag_of: datatypes.get("tag_of").lua()?,
            set_type: datatypes.get("set_type").lua()?,
            callbacks: lua.create_table().lua()?,
            running: RefCell::new(Default::default()),
            chunks: RefCell::new(Default::default()),
            compiler: script_compiler(),
        });
        api::install_globals(&lua, &ctx).lua()?;
        let mkinput: Function = exports.get("mkinput").lua()?;
        let reap: Function = exports.get("reap").lua()?;
        let reset_modules: Function = exports.get("reset_modules").lua()?;
        let profiler = ProfilerFns {
            set: exports.get("set_profiling").lua()?,
            adopt: exports.get("adopt").lua()?,
            charge_thread: exports.get("charge_thread").lua()?,
            charge_script: exports.get("charge_script").lua()?,
            take: exports.get("take_costs").lua()?,
        };
        globals.set("__log", Value::Nil).lua()?;

        let hot = Hot::new(&ctx).lua()?;

        lua.sandbox(true).lua()?;
        lua.set_memory_limit(768 * 1024 * 1024).lua()?;

        let budget = PHASE_BUDGET;
        let deadline = install_time_budget(&lua, budget);

        // The simulation listens to this data model from now on (everything in it is looked at by its first step).
        ctx.physics.borrow_mut().attach(&mut ctx.dm.borrow_mut());

        Ok(Self { lua, ctx, mkinput, reap, reset_modules, hot, deadline, budget, log, profiler, started: Cell::new(false), profiling: Cell::new(false), profile: RefCell::new(ScriptProfile::default()), physics_ms: Cell::new(0.0) })
    }

    pub fn dm(&self) -> Rc<RefCell<DataModel>> {
        self.ctx.dm.clone()
    }

    // ---- physics ----------------------------------------------------------------------

    /// Brings the simulation up to date with the data model and steps the systems that run before the world
    /// does. Called from `FixedUpdate`, before `Box3dSet::Step`.
    pub fn physics_pre_step(&self, world: &box3d::World, dt: f32) {
        let started = Instant::now();
        self.ctx.physics.borrow_mut().pre_step(&mut self.ctx.dm.borrow_mut(), world, dt);
        self.physics_ms.set(self.physics_ms.get() + started.elapsed().as_secs_f32() * 1000.0);
    }

    /// Writes what the world did into the data model and turns touches into events. Called from `FixedUpdate`,
    /// after `Box3dSet::Step`; `step_ms` is how long the step itself took.
    pub fn physics_post_step(&self, world: &box3d::World, step_ms: f32) {
        let started = Instant::now();
        self.ctx.physics.borrow_mut().post_step(&mut self.ctx.dm.borrow_mut(), world);
        self.physics_ms.set(self.physics_ms.get() + step_ms + started.elapsed().as_secs_f32() * 1000.0);
    }
    // ---- profiling ---------------------------------------------------------------------

    /// Switches the measuring of each script's cost on or off. (How long each phase of a frame took
    /// is always measured.) Off by default: it adds two clock reads to every handler that runs.
    pub fn set_profiling(&self, on: bool) {
        if self.profiling.replace(on) != on {
            if let Err(e) = self.profiler.set.call::<()>(on) {
                self.log.emit(LogLevel::Error, format!("[script] could not switch profiling: {e}"));
            }
            self.profile.borrow_mut().per_script = on;
        }
    }

    /// What the last frames cost (see [`ScriptProfile`]).
    pub fn profile(&self) -> ScriptProfile {
        self.profile.borrow().clone()
    }

    /// What the last frame of scripts and of physics took, in milliseconds `(scripts, physics)`:
    /// a few numbers, cheap enough to read every frame.
    pub fn frame_ms(&self) -> (f32, f32) {
        let p = self.profile.borrow();
        (p.scripts_ms(), p.physics_ms())
    }

    /// What the last physics step did.
    pub fn physics_stats(&self) -> crate::physics::PhysicsStats {
        let mut stats = self.ctx.physics.borrow().stats.clone();
        stats.step_ms = self.physics_ms.get();
        stats
    }

    /// Forgets the totals and peaks collected so far.
    pub fn clear_profile(&self) {
        self.profile.borrow_mut().clear_totals();
    }

    /// Takes what the prelude charged during the frame into the profile.
    fn collect_costs(&self) {
        let taken: Table = match self.profiler.take.call(()) {
            Ok(t) => t,
            Err(_) => return,
        };
        let seen: Vec<(u32, f64, u32)> = taken
            .pairs::<u32, Table>()
            .flatten()
            .map(|(uid, entry)| (uid, entry.get::<f64>(1).unwrap_or(0.0), entry.get::<u32>(2).unwrap_or(0)))
            .collect();
        let dm = self.ctx.dm.borrow();
        // Scripts not seen before are looked up together, in one pass over the data model
        // (and only if there are any): a search per script is a stall with thousands of them.
        let mut found: Option<FastMap<u32, Id>> = None;
        self.profile.borrow_mut().absorb(&seen, |uid| {
            let found = found.get_or_insert_with(|| dm.ids_of_uids(&seen.iter().map(|s| s.0).collect()));
            match found.get(&uid).copied() {
                Some(id) if dm.has_component(id, "ScriptRunner") => (dm.name(id).to_owned(), dm.string(id, "SourceFile")),
                Some(id) => (dm.name(id).to_owned(), String::new()),
                None => (format!("(destroyed script {uid})"), String::new()),
            }
        });
    }

    /// Charges `started.elapsed()` to the script `thread` belongs to (only while profiling).
    fn charge_thread(&self, thread: &Thread, started: Instant) {
        if self.profiling.get() {
            let _ = self.profiler.charge_thread.call::<()>((thread.clone(), started.elapsed().as_secs_f64()));
        }
    }

    /// Starts the budget clock for the code that is about to run.
    fn arm(&self) {
        self.deadline.set(Instant::now() + self.budget);
    }

    /// Records script output for [`ScriptHost::drain_logs`] (the process log always gets it).
    /// Off by default so a game that never reads it does not accumulate lines.
    pub fn capture_logs(&self, on: bool) {
        self.log.capture.set(on);
        if !on {
            self.log.lines.borrow_mut().clear();
        }
    }

    /// Everything scripts printed since the last call (only while `capture_logs(true)`).
    pub fn drain_logs(&self) -> Vec<LogLine> {
        std::mem::take(&mut *self.log.lines.borrow_mut())
    }

    fn report(&self, what: &str, r: LuaResult<()>) {
        if let Err(e) = r {
            self.log.emit(LogLevel::Error, format!("[script] {what}: {e}"));
        }
    }

    /// Queues every script in the data model to be started by the next frame (for a host that
    /// attaches to an already-loaded data model, like the editor's Play mode).
    pub fn start_all(&self) {
        let mut dm = self.ctx.dm.borrow_mut();
        let all = dm.with_component("ScriptRunner");
        dm.pending_scripts.extend(all);
    }

    /// Replaces the contents of the game with a scene and restarts scripting.
    pub fn load_place(&mut self, place: &PlaceFile) -> Result<()> {
        self.ctx.dm.borrow_mut().load_place(place).map_err(|e| anyhow::anyhow!(e))?;
        for w in std::mem::take(&mut self.ctx.dm.borrow_mut().load_warnings) {
            self.log.emit(LogLevel::Warn, format!("[scene] {w}"));
        }
        self.reset_state()?;
        Ok(())
    }

    fn reset_state(&mut self) -> Result<()> {
        let running: Vec<(u32, Thread)> = self.ctx.running.borrow_mut().drain().collect();
        for (uid, thread) in running {
            self.stop_started(uid, Some(thread));
        }
        self.ctx.sched.borrow_mut().clear();
        self.reset_modules.call::<()>(()).lua()?;
        self.ctx.signals.borrow_mut().clear();
        for uid in self.ctx.callbacks.pairs::<u32, Value>().flatten().map(|(uid, _)| uid).collect::<Vec<_>>() {
            self.ctx.callbacks.raw_remove(uid).lua()?;
        }
        {
            let mut dm = self.ctx.dm.borrow_mut();
            dm.watched.clear();
            dm.touch_watch.clear();
            dm.events.clear();
        }
        // (The old simulation is dropped: its bodies and shapes are destroyed. The new one starts from the scene.)
        {
            let mut physics = self.ctx.physics.borrow_mut();
            physics.reset();
            physics.attach(&mut self.ctx.dm.borrow_mut());
        }
        self.hot = Hot::new(&self.ctx).lua()?;
        self.started.set(false);
        Ok(())
    }

    /// What the scripts asked of the host since the last call (closing the game, another scene). A closing game first runs
    /// the handlers of `game:BindToClose`.
    pub fn take_requests(&self) -> Requests {
        let requests = std::mem::take(&mut *self.ctx.requests.borrow_mut());
        if requests.quit {
            self.arm();
            if let Ok(Some(run)) = self.lua.globals().get::<Option<Function>>("__run_close") {
                self.report("BindToClose", run.call::<()>(()));
            }
        }
        requests
    }

    /// Tells the cameras how big the picture is (`Camera.ViewportSize`), in pixels: the host calls it when the window or
    /// the viewport changes.
    pub fn set_viewport(&self, size: Vec2) {
        let mut dm = self.ctx.dm.borrow_mut();
        let Some(camera) = dm.reference(dm.workspace, "CurrentCamera") else { return };
        if size.x >= 1.0 && size.y >= 1.0 {
            let _ = dm.set_prop(camera, "ViewportSize", crate::core::Value::V2(size));
        }
    }

    pub fn mouse_behavior(&self) -> String {
        let dm = self.ctx.dm.borrow();
        dm.service("UserInputService").map(|u| dm.string(u, "MouseBehavior")).unwrap_or_default()
    }

    // ---- scripts ----------------------------------------------------------------------

    fn start_pending(&self) {
        let pending = std::mem::take(&mut self.ctx.dm.borrow_mut().pending_scripts);
        for id in pending {
            let (path, ok, uid, name) = {
                let dm = self.ctx.dm.borrow();
                let Some(inst) = dm.get(id) else { continue };
                if !inst.class.has_component("ScriptRunner") {
                    continue;
                }
                let runnable = dm.services_where(|c| c.desc.runs_scripts).into_iter().any(|sid| dm.is_descendant_of(id, sid));
                (dm.string(id, "SourceFile"), runnable && dm.flag(id, "Enabled"), inst.uid, inst.name.clone())
            };
            let started = self.ctx.running.borrow().contains_key(&uid);
            if started && !ok {
                // Disabled, or moved out of the places scripts run in: stop what it started.
                let thread = self.ctx.running.borrow_mut().remove(&uid);
                self.stop_started(uid, thread);
                continue;
            }
            if !ok || path.is_empty() || started {
                continue;
            }
            self.arm();
            if let Err(e) = self.run_script(id, uid, &path) {
                self.log.emit(LogLevel::Error, format!("[script] {name} ({path}): {e}"));
            }
        }
    }

    /// Tells the services that the game is running, once, after the scene's scripts have been started (a
    /// scene that replaces this one starts it again): the Players service lets the player join, for one.
    fn notify_started(&self) {
        if self.started.replace(true) {
            return;
        }
        let services: Vec<(Id, &'static crate::core::ResolvedClass)> = {
            let mut dm = self.ctx.dm.borrow_mut();
            let all: Vec<(Id, &'static crate::core::ResolvedClass)> = dm.children(dm.game).into_iter().filter_map(|s| dm.get(s).map(|i| (s, i.class))).collect();
            for (service, class) in &all {
                class.desc.behavior.on_started(&mut dm, *service);
            }
            all
        };
        // A service written in Luau starts its own work (listening for input ...) with a `__start` method.
        self.arm();
        for (service, class) in services {
            let Ok(Some(start)) = self.ctx.lua_method(class, "__start") else { continue };
            let result = self.ctx.wrap(&self.lua, service).and_then(|ud| start.call::<()>(ud));
            self.report("service start", result);
        }
    }

    fn run_script(&self, id: Id, uid: u32, path: &str) -> LuaResult<()> {
        let ud = self.ctx.wrap(&self.lua, id)?;
        let f = self.ctx.load_chunk(&self.lua, path, ud)?;
        let thread = self.lua.create_thread(f)?;
        self.ctx.running.borrow_mut().insert(uid, thread.clone());
        // (Later resumes of this thread, after a `task.wait`, are charged to the script too.)
        self.profiler.adopt.call::<()>((thread.clone(), uid))?;
        let started = Instant::now();
        let result = thread.resume::<()>(());
        if self.profiling.get() {
            let _ = self.profiler.charge_script.call::<()>((uid, started.elapsed().as_secs_f64()));
        }
        result
    }

    /// Ends everything the script with this uid started: its connections and its threads.
    fn stop_started(&self, uid: u32, main: Option<Thread>) {
        let main = main.map_or(Value::Nil, Value::Thread);
        if let Err(e) = self.reap.call::<()>((uid, main)) {
            self.log.emit(LogLevel::Error, format!("[script] could not stop a script: {e}"));
        }
    }

    /// Cleans up after instances that were destroyed: stops their scripts and forgets their signals.
    /// (Run after the frame's events were delivered, so `Destroying` handlers still see them.)
    fn reap_dead(&self) {
        let (scripts, watched) = {
            let mut dm = self.ctx.dm.borrow_mut();
            dm.graves.clear();
            (std::mem::take(&mut dm.dead_scripts), std::mem::take(&mut dm.dead_watched))
        };
        for uid in scripts {
            let thread = self.ctx.running.borrow_mut().remove(&uid);
            self.stop_started(uid, thread);
        }
        self.ctx.forget_signals(&watched);
    }

    // ---- frame ----------------------------------------------------------------------------

    pub fn frame(&mut self, f: &FrameInput) {
        self.frame_with(f, None);
    }

    /// Runs one frame of scripts. With `world` (the Box3D world the game's physics steps), the methods scripts call
    /// can reach the simulation for the length of the frame: impulses, ray casts, overlap tests.
    pub fn frame_with(&mut self, f: &FrameInput, world: Option<&box3d::World>) {
        let ctx = self.ctx.clone();
        let _scope = world.map(|w| WorldScope::enter(ctx.physics.borrow().slot(), w));
        ctx.time.set(ctx.time.get() + f.dt as f64);
        ctx.dt.set(f.dt);
        {
            let mut dm = ctx.dm.borrow_mut();
            let workspace = dm.workspace;
            let _ = dm.set_prop(workspace, "DistributedGameTime", crate::core::Value::Num(ctx.time.get() as f32));
        }
        {
            let mut i = ctx.input.borrow_mut();
            i.keys.clone_from(f.keys);
            i.mouse.clone_from(f.mouse);
            i.delta = f.mouse_delta;
            if let Some(c) = f.cursor {
                i.pos = c;
            }
        }
        // Every phase starts its own time budget (see `PHASE_BUDGET`). `lap` times the phases
        // for the debug overlays.
        let mut lap = Lap::new();
        self.arm();
        self.start_pending();
        self.notify_started();
        lap.to(P_START);
        self.arm();
        self.report("input", self.fire_input(f));
        lap.to(P_INPUT);
        self.arm();
        self.report("Stepped", ctx.fire2(&self.hot.stepped, ctx.time.get(), f.dt));
        self.report("PreAnimation", ctx.fire1(&self.hot.pre_animation, f.dt));
        self.report("PreSimulation", ctx.fire1(&self.hot.pre_simulation, f.dt));
        lap.to(P_STEPPED);
        self.arm();
        self.dispatch_events();
        lap.to(P_EVENTS);

        // (The physics ran in `FixedUpdate` just before this frame; what it cost is charged to its phase.)
        lap.ms[P_PHYSICS] += self.physics_ms.take();
        ctx.physics.borrow_mut().end_frame();

        self.arm();
        self.report("PostSimulation", ctx.fire1(&self.hot.post_simulation, f.dt));
        self.report("Heartbeat", ctx.fire1(&self.hot.heartbeat, f.dt));
        lap.to(P_HEARTBEAT);
        self.arm();
        self.resume_due();
        lap.to(P_TIMERS);
        self.arm();
        self.dispatch_events();
        self.start_pending();
        lap.to(P_EVENTS);

        self.arm();
        self.report("PreRender", ctx.fire1(&self.hot.pre_render, f.dt));
        self.report("RenderStepped", ctx.fire1(&self.hot.render_stepped, f.dt));
        lap.to(P_RENDER_STEPPED);
        self.arm();
        self.dispatch_events();
        self.reap_dead();
        lap.to(P_EVENTS);

        {
            let mut profile = self.profile.borrow_mut();
            profile.phase_ms = lap.ms;
            profile.frame_ms = lap.ms.iter().sum();
            profile.frames += 1;
        }
        if self.profiling.get() {
            self.collect_costs();
        }
    }

    fn resume_due(&self) {
        let now = self.ctx.time.get();
        let due = self.ctx.sched.borrow_mut().take_due(now);
        for w in due {
            if w.thread.status() == ThreadStatus::Resumable {
                let started = Instant::now();
                if let Err(e) = w.thread.resume::<()>(now - w.started) {
                    self.log.emit(LogLevel::Error, format!("[script] {e}"));
                }
                self.charge_thread(&w.thread, started);
            }
        }
    }

    fn fire_input(&self, f: &FrameInput) -> LuaResult<()> {
        let ctx = &self.ctx;
        let mk = |key: Option<&str>, uit: &str, state: &str, d: (f32, f32, f32)| -> LuaResult<Table> { self.mkinput.call((key, uit, state, d.0, d.1, d.2)) };
        for k in f.keys.get_just_pressed() {
            if let Some(name) = keys::BY_CODE.get(k) {
                ctx.fire2(&self.hot.began, mk(Some(*name), "Keyboard", "Begin", (0.0, 0.0, 0.0))?, false)?;
            }
        }
        for k in f.keys.get_just_released() {
            if let Some(name) = keys::BY_CODE.get(k) {
                ctx.fire2(&self.hot.ended, mk(Some(*name), "Keyboard", "End", (0.0, 0.0, 0.0))?, false)?;
            }
        }
        // The jump key: UserInputService.JumpRequest.
        if f.keys.just_pressed(KeyCode::Space) {
            if let Some(uis) = ctx.dm.borrow().service("UserInputService").and_then(|s| ctx.dm.borrow().get(s).map(|i| i.uid)) {
                if let Some(signal) = ctx.existing_signal(uis, "JumpRequest") {
                    ctx.fire0(&signal)?;
                }
            }
        }
        for (name, b) in keys::MOUSE {
            if f.mouse.just_pressed(b) {
                ctx.fire2(&self.hot.began, mk(None, name, "Begin", (0.0, 0.0, 0.0))?, false)?;
            }
            if f.mouse.just_released(b) {
                ctx.fire2(&self.hot.ended, mk(None, name, "End", (0.0, 0.0, 0.0))?, false)?;
            }
        }
        if f.mouse_delta != Vec2::ZERO {
            ctx.fire2(&self.hot.changed, mk(None, "MouseMovement", "Change", (f.mouse_delta.x, f.mouse_delta.y, 0.0))?, false)?;
        }
        if f.wheel != 0.0 {
            ctx.fire2(&self.hot.changed, mk(None, "MouseWheel", "Change", (0.0, 0.0, f.wheel))?, false)?;
        }
        Ok(())
    }

    /// Tells `LogService.MessageOut` handlers about the lines printed since the last time.
    fn deliver_log(&self) {
        let lines = std::mem::take(&mut *self.log.feed.borrow_mut());
        if lines.is_empty() {
            return;
        }
        let Some(service) = self.ctx.dm.borrow().service("LogService") else { return };
        let Some(signal) = self.ctx.existing_signal(self.ctx.dm.borrow().get(service).map_or(0, |i| i.uid), "MessageOut") else { return };
        let Ok(types) = self.ctx.enums.raw_get::<Table>("MessageType") else { return };
        for (level, text) in lines {
            let item = match level {
                LogLevel::Info => "MessageOutput",
                LogLevel::Warn => "MessageWarning",
                LogLevel::Error => "MessageError",
            };
            let Ok(item) = types.raw_get::<Table>(item) else { continue };
            if let Err(e) = self.ctx.signal_fire.call::<()>((signal.clone(), text, item)) {
                warn!(target: "script", "[script] LogService: {e}");
            }
        }
    }

    fn dispatch_events(&self) {
        self.deliver_log();
        for _ in 0..8 {
            let events = std::mem::take(&mut self.ctx.dm.borrow_mut().events);
            if events.is_empty() {
                break;
            }
            for ev in events {
                if let Err(e) = self.fire_event(ev) {
                    self.log.emit(LogLevel::Error, format!("[script] {e}"));
                }
            }
        }
    }

    fn fire_event(&self, ev: Event) -> LuaResult<()> {
        let c = &self.ctx;
        match ev {
            Event::Changed(id, prop, carried) => {
                if !c.has_signals(id.uid) {
                    return Ok(());
                }
                if let Some(sig) = c.existing_signal(id.uid, "Changed") {
                    let arg = match carried {
                        Some((ty, v)) => c.to_lua(&self.lua, ty, &v)?,
                        None => Value::String(self.lua.create_string(prop)?),
                    };
                    c.fire1(&sig, arg)?;
                }
                if let Some(sig) = c.existing_signal(id.uid, &format!("Changed:{prop}")) {
                    c.fire0(&sig)?;
                }
            }
            Event::ChildAdded(parent, child) => {
                if let Some(sig) = c.existing_signal(parent.uid, "ChildAdded") {
                    c.fire1(&sig, c.wrap(&self.lua, child)?)?;
                }
            }
            Event::ChildRemoved(parent, child) => {
                if let Some(sig) = c.existing_signal(parent.uid, "ChildRemoved") {
                    c.fire1(&sig, c.wrap(&self.lua, child)?)?;
                }
            }
            Event::Destroying(id) => {
                if let Some(sig) = c.existing_signal(id.uid, "Destroying") {
                    c.fire0(&sig)?;
                }
            }
            Event::Touched(part, other) => {
                if let Some(sig) = c.existing_signal(part.uid, "Touched") {
                    c.fire1(&sig, c.wrap(&self.lua, other)?)?;
                }
            }
            Event::TouchEnded(part, other) => {
                if let Some(sig) = c.existing_signal(part.uid, "TouchEnded") {
                    c.fire1(&sig, c.wrap(&self.lua, other)?)?;
                }
            }
            // Any other signal of any class (see `DataModel::fire`): the arguments come as script values.
            Event::Signal(id, name, args) => {
                if let Some(sig) = c.existing_signal(id.uid, &name) {
                    let mut call = Vec::with_capacity(args.len() + 1);
                    call.push(Value::Table(sig));
                    for arg in args {
                        call.push(c.script_to_lua(&self.lua, arg)?);
                    }
                    c.signal_fire.call::<()>(mlua::MultiValue::from_iter(call))?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Runs real Luau through the host against a real data model.

    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::core::Value as DV;

    struct Game {
        dm: Rc<RefCell<DataModel>>,
        host: ScriptHost,
        dir: std::path::PathBuf,
        keys: ButtonInput<KeyCode>,
        mouse: ButtonInput<MouseButton>,
    }

    impl Drop for Game {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    impl Game {
        /// A game with the given script files (`name.luau` -> source) and an empty data model.
        fn new(files: &[(&str, &str)]) -> Game {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_script_test_{}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            for (name, src) in files {
                if let Some(parent) = dir.join(name).parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(dir.join(name), src).unwrap();
            }
            let dm = Rc::new(RefCell::new(DataModel::new()));
            let host = ScriptHost::new(GameSource::Dir(dir.clone()), dm.clone()).expect("host");
            Game { dm, host, dir, keys: ButtonInput::default(), mouse: ButtonInput::default() }
        }

        /// Creates an instance of `class` named `name` under the service `parent`.
        fn add(&self, class: &str, name: &str, parent: &str, props: &[(&str, DV)]) -> Id {
            let mut dm = self.dm.borrow_mut();
            let id = dm.create(class).unwrap();
            dm.set_name(id, name);
            for (k, v) in props {
                dm.set_prop(id, k, v.clone()).unwrap();
            }
            let parent = dm.service(parent).unwrap();
            dm.set_parent(id, Some(parent)).unwrap();
            id
        }

        fn script(&self, name: &str, file: &str) -> Id {
            self.add("Script", name, "ServerScriptService", &[("SourceFile", DV::Str(file.into()))])
        }

        fn number(&self, name: &str) -> Id {
            self.add("NumberValue", name, "ReplicatedStorage", &[])
        }

        fn value(&self, id: Id) -> f32 {
            self.dm.borrow().num(id, "Value", f32::NAN)
        }

        fn text(&self, id: Id) -> String {
            self.dm.borrow().string(id, "Value")
        }

        fn frames(&mut self, n: usize) {
            for _ in 0..n {
                let input = FrameInput { dt: 1.0 / 60.0, keys: &self.keys, mouse: &self.mouse, mouse_delta: Vec2::ZERO, cursor: None, wheel: 0.0 };
                self.host.frame(&input);
            }
        }
    }

    const COUNT_FRAMES: &str = r#"
        local counter = game:GetService("ReplicatedStorage").Frames
        game:GetService("RunService").Heartbeat:Connect(function() counter.Value += 1 end)
    "#;

    const COUNT_LOOPS: &str = r#"
        local counter = game:GetService("ReplicatedStorage").Loops
        while true do
            counter.Value += 1
            task.wait()
        end
    "#;

    #[test]
    fn a_script_runs_and_prints_through_the_host() {
        let mut g = Game::new(&[("hello.luau", "game:GetService('ReplicatedStorage').Out.Value = 'hi from ' .. script.Name")]);
        g.host.capture_logs(true);
        let out = g.add("StringValue", "Out", "ReplicatedStorage", &[]);
        g.script("Hello", "hello.luau");
        g.host.start_all();
        g.frames(2);
        assert_eq!(g.text(out), "hi from Hello");
    }

    #[test]
    fn destroying_a_script_stops_its_connections_and_threads() {
        let mut g = Game::new(&[("frames.luau", COUNT_FRAMES), ("loops.luau", COUNT_LOOPS)]);
        let frames = g.number("Frames");
        let loops = g.number("Loops");
        let a = g.script("Frames", "frames.luau");
        let b = g.script("Loops", "loops.luau");
        g.host.start_all();
        g.frames(10);
        assert!(g.value(frames) >= 8.0, "Heartbeat handler runs, got {}", g.value(frames));
        assert!(g.value(loops) >= 8.0, "the task.wait loop runs, got {}", g.value(loops));

        g.dm.borrow_mut().destroy(a);
        g.dm.borrow_mut().destroy(b);
        g.frames(2); // the frame that notices the destruction
        let (f, l) = (g.value(frames), g.value(loops));
        g.frames(30);
        assert_eq!(g.value(frames), f, "a destroyed script's connection must be gone");
        assert_eq!(g.value(loops), l, "a destroyed script's thread must be gone");
    }

    #[test]
    fn disabling_a_script_stops_it_and_enabling_restarts_it() {
        let mut g = Game::new(&[("frames.luau", COUNT_FRAMES)]);
        let frames = g.number("Frames");
        let s = g.script("Frames", "frames.luau");
        g.host.start_all();
        g.frames(5);
        g.dm.borrow_mut().set_prop(s, "Enabled", DV::Bool(false)).unwrap();
        g.frames(2);
        let stopped = g.value(frames);
        g.frames(20);
        assert_eq!(g.value(frames), stopped, "a disabled script does not run");

        g.dm.borrow_mut().set_prop(s, "Enabled", DV::Bool(true)).unwrap();
        g.frames(10);
        assert!(g.value(frames) > stopped, "enabling the script starts it again");
    }

    #[test]
    fn scripts_sharing_a_file_are_compiled_once_but_each_keeps_its_own_identity() {
        const TICK: &str = "local v = game:GetService('ReplicatedStorage')[script.Name] game:GetService('RunService').Heartbeat:Connect(function() v.Value += 1 end)";
        let mut g = Game::new(&[("tick.luau", TICK)]);
        let (a, b, c) = (g.number("A"), g.number("B"), g.number("C"));
        let (script_a, _, _) = (g.script("A", "tick.luau"), g.script("B", "tick.luau"), g.script("C", "tick.luau"));
        g.host.start_all();
        g.frames(5);
        assert!(g.value(a) >= 3.0 && g.value(b) >= 3.0 && g.value(c) >= 3.0, "each instance runs with its own `script`: {} {} {}", g.value(a), g.value(b), g.value(c));
        assert_eq!(g.host.ctx.chunks.borrow().len(), 1, "one file, one compile");

        // Chunk names carry the instance, so destroying one script stops only what that one started.
        g.dm.borrow_mut().destroy(script_a);
        g.frames(2);
        let (frozen, running) = (g.value(a), g.value(b));
        g.frames(10);
        assert_eq!(g.value(a), frozen, "the destroyed script's handler is gone");
        assert!(g.value(b) > running, "the others carry on");
    }

    #[test]
    fn a_script_file_that_changed_is_compiled_again_and_one_that_does_not_compile_is_reported_as_before() {
        let mut g = Game::new(&[("v.luau", "game:GetService('ReplicatedStorage').Out.Value = 'one'"), ("bad.luau", "local = 1")]);
        g.host.capture_logs(true);
        let out = g.add("StringValue", "Out", "ReplicatedStorage", &[]);
        g.script("First", "v.luau");
        g.host.start_all();
        g.frames(2);
        assert_eq!(g.text(out), "one");

        std::fs::write(g.dir.join("v.luau"), "game:GetService('ReplicatedStorage').Out.Value = 'the second version'").unwrap();
        g.script("Second", "v.luau");
        g.host.start_all();
        g.frames(2);
        assert_eq!(g.text(out), "the second version", "a script started after the file was edited runs the edited file");

        g.script("Broken", "bad.luau");
        g.host.start_all();
        g.frames(2);
        let errors: Vec<String> = g.host.drain_logs().into_iter().filter(|l| l.level == LogLevel::Error).map(|l| l.text).collect();
        assert!(errors.iter().any(|e| e.contains("Broken") && e.contains("bad.luau#")), "the error names the chunk, as when source was loaded directly: {errors:?}");
        assert!(!g.host.ctx.chunks.borrow().contains_key("bad.luau"), "only scripts that compile are kept");

        // A good file that is edited into a bad one: the old bytecode must not be served again, not even to
        // a second script started in the same frame (which does not look at the file again).
        std::fs::write(g.dir.join("v.luau"), "local = oops").unwrap();
        g.script("Third", "v.luau");
        g.script("Fourth", "v.luau");
        g.host.start_all();
        g.frames(2);
        let errors: Vec<String> = g.host.drain_logs().into_iter().filter(|l| l.level == LogLevel::Error).map(|l| l.text).collect();
        assert!(errors.iter().any(|e| e.contains("Third")) && errors.iter().any(|e| e.contains("Fourth")), "both report the error: {errors:?}");
        assert!(!g.host.ctx.chunks.borrow().contains_key("v.luau"));
    }

    #[test]
    fn profiling_charges_time_to_the_script_that_spent_it() {
        let busy = "game:GetService('RunService').Heartbeat:Connect(function() local x = 0 for i = 1, 400000 do x += math.sqrt(i) end end)";
        let light = "game:GetService('RunService').Heartbeat:Connect(function() end)";
        // Work done after a `task.wait` belongs to the script too (the host resumes that thread itself).
        let waiter = "while true do task.wait() local x = 0 for i = 1, 400000 do x += math.sqrt(i) end end";
        let mut g = Game::new(&[("busy.luau", busy), ("light.luau", light), ("waiter.luau", waiter)]);
        g.script("Busy", "busy.luau");
        g.script("Light", "light.luau");
        g.script("Waiter", "waiter.luau");
        g.host.set_profiling(true);
        g.host.start_all();
        g.frames(12);

        let p = g.host.profile();
        assert!(p.per_script);
        let cost = |name: &str| p.scripts.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no cost recorded for {name}: {:?}", p.scripts)).clone();
        let (busy, light, waiter) = (cost("Busy"), cost("Light"), cost("Waiter"));
        assert!(busy.total_ms > 5.0 * light.total_ms.max(0.01), "the busy script is far more expensive: busy {} ms, light {} ms", busy.total_ms, light.total_ms);
        assert!(waiter.total_ms > 1.0, "time after task.wait is charged to the script, got {} ms", waiter.total_ms);
        assert!(busy.calls >= 10 && light.calls >= 10, "every handler run is counted: {} / {}", busy.calls, light.calls);
        assert_eq!(busy.path, "busy.luau");
        assert!(p.by_cost().windows(2).all(|w| w[0].avg_ms >= w[1].avg_ms), "most expensive first");
        assert!(p.phase_ms[super::P_HEARTBEAT] > 0.0 && p.frame_ms >= p.phase_ms[super::P_HEARTBEAT], "phases are timed");

        // Switched off: nothing more is collected.
        g.host.set_profiling(false);
        let before = g.host.profile().scripts.iter().map(|s| s.total_ms).sum::<f64>();
        g.frames(5);
        assert_eq!(g.host.profile().scripts.iter().map(|s| s.total_ms).sum::<f64>(), before);
        assert!(g.host.profile().frames >= 17, "phase timing keeps running");
    }

    #[test]
    fn a_table_that_contains_itself_is_an_error_not_a_crash() {
        let src = r#"
            local t = {}
            t[1] = t
            local ok, err = pcall(function() workspace:Raycast(Vector3.new(0, 0, 0), Vector3.new(0, -1, 0), { FilterDescendantsInstances = t }) end)
            game:GetService("ReplicatedStorage").Out.Value = ok and "no error" or tostring(err)
        "#;
        let mut g = Game::new(&[("cycle.luau", src)]);
        let out = g.add("StringValue", "Out", "ReplicatedStorage", &[]);
        g.script("Cycle", "cycle.luau");
        g.host.start_all();
        g.frames(2);
        assert!(g.text(out).contains("nested too deeply"), "got: {}", g.text(out));
    }

    #[test]
    fn scripts_and_modules_run_from_any_folder_and_the_folder_decides_nothing() {
        // The module is in a folder that sounds like client code and the script in one that sounds like
        // shared types: what runs, and how, is up to the objects, not to where their files are.
        let mut g = Game::new(&[
            ("Client/UI/Deep/lib.luau", "local M = {} function M.double(x) return x * 2 end return M"),
            ("Shared/Types/main.luau", "game:GetService('ReplicatedStorage').Out.Value = require(script.Parent.Lib).double(21)"),
        ]);
        let out = g.number("Out");
        g.add("ModuleScript", "Lib", "ServerScriptService", &[("SourceFile", DV::Str("Client/UI/Deep/lib.luau".into()))]);
        g.script("Main", "Shared/Types/main.luau");
        g.host.start_all();
        g.frames(2);
        assert_eq!(g.value(out), 42.0);

        // The module's file moved elsewhere and its object points at the new place: same result.
        let mut g = Game::new(&[
            ("Anywhere/At/All/renamed.luau", "local M = {} function M.double(x) return x * 2 end return M"),
            ("main.luau", "game:GetService('ReplicatedStorage').Out.Value = require(script.Parent.Lib).double(21)"),
        ]);
        let out = g.number("Out");
        g.add("ModuleScript", "Lib", "ServerScriptService", &[("SourceFile", DV::Str("Anywhere/At/All/renamed.luau".into()))]);
        g.script("Main", "main.luau");
        g.host.start_all();
        g.frames(2);
        assert_eq!(g.value(out), 42.0);
    }

    #[test]
    fn cyclic_require_is_reported() {
        let mut g = Game::new(&[
            ("a.luau", "return require(script.Parent.B)"),
            ("b.luau", "return require(script.Parent.A)"),
            ("main.luau", "local ok, err = pcall(require, script.Parent.A); game:GetService('ReplicatedStorage').Out.Value = ok and 'loaded' or tostring(err)"),
        ]);
        let out = g.add("StringValue", "Out", "ReplicatedStorage", &[]);
        let sss = g.dm.borrow().service("ServerScriptService").unwrap();
        for (class, name, file) in [("ModuleScript", "A", "a.luau"), ("ModuleScript", "B", "b.luau")] {
            let id = {
                let mut dm = g.dm.borrow_mut();
                let id = dm.create(class).unwrap();
                dm.set_name(id, name);
                dm.set_prop(id, "SourceFile", DV::Str(file.into())).unwrap();
                dm.set_parent(id, Some(sss)).unwrap();
                id
            };
            let _ = id;
        }
        g.add("Script", "Main", "ServerScriptService", &[("SourceFile", DV::Str("main.luau".into()))]);
        g.host.start_all();
        g.frames(2);
        assert!(g.text(out).contains("cyclic require"), "got: {}", g.text(out));
    }

    #[test]
    fn collection_service_tags_do_not_depend_on_script_references() {
        let tag = r#"
            local CS = game:GetService("CollectionService")
            CS:AddTag(workspace.Thing, "Coin")
            collectgarbage()
        "#;
        let check = r#"
            local CS = game:GetService("CollectionService")
            game:GetService("ReplicatedStorage").Out.Value = #CS:GetTagged("Coin") .. ":" .. tostring(CS:HasTag(workspace.Thing, "Coin"))
        "#;
        let mut g = Game::new(&[("tag.luau", tag), ("check.luau", check)]);
        let out = g.add("StringValue", "Out", "ReplicatedStorage", &[]);
        g.add("Folder", "Thing", "Workspace", &[]);
        g.script("Tag", "tag.luau");
        g.host.start_all();
        g.frames(3);
        for _ in 0..3 {
            collect_garbage(&g.host);
        }
        g.script("Check", "check.luau");
        g.host.start_all();
        g.frames(2);
        assert_eq!(g.text(out), "1:true");
    }

    #[test]
    fn scripts_see_the_pivot_offset_through_getpivot_and_pivotto() {
        let src = r#"
            local part = workspace.P
            part.PivotOffset = Vector3.new(2, 0, 0)
            local at = part:GetPivot().Position
            part:PivotTo(CFrame.new(0, 5, 0))
            game:GetService("ReplicatedStorage").Out.Value = at.X .. "," .. part.Position.X .. "," .. part.Position.Y
        "#;
        let mut g = Game::new(&[("pivot.luau", src)]);
        let out = g.add("StringValue", "Out", "ReplicatedStorage", &[]);
        g.add("Part", "P", "Workspace", &[("Position", DV::V3(bevy::math::Vec3::new(10.0, 0.0, 0.0)))]);
        g.script("Pivot", "pivot.luau");
        g.host.start_all();
        g.frames(2);
        assert_eq!(g.text(out), "12,-2,5", "the pivot was 2 studs along the part, so putting it at x=0 puts the centre at x=-2");
    }

    #[test]
    fn changed_signals_fire_for_real_changes_only() {
        let src = r#"
            local out = game:GetService("ReplicatedStorage").Out
            local p = workspace.P
            local size, any = 0, {}
            p:GetPropertyChangedSignal("Size"):Connect(function() size += 1 end)
            p.Changed:Connect(function(name) table.insert(any, name) end)
            p.Size = p.Size                      -- the value it has: not a change
            p.Color = p.Color
            p.Size = Vector3.new(1, 2, 3)        -- a change
            p.Size = Vector3.new(1, 2, 3)        -- the same again: not a change
            p.Color = Color3.new(1, 0, 0)        -- a change to something else
            task.wait()
            task.wait()
            out.Value = size .. ":" .. table.concat(any, ",")
        "#;
        let mut g = Game::new(&[("changed.luau", src)]);
        let out = g.add("StringValue", "Out", "ReplicatedStorage", &[]);
        g.add("Part", "P", "Workspace", &[("Anchored", DV::Bool(true))]); // (a falling part would report its CFrame too)
        g.script("Changed", "changed.luau");
        g.host.start_all();
        g.frames(5);
        assert_eq!(g.text(out), "1:Size,Color");
    }

    #[test]
    fn a_handler_that_yields_ends_with_its_script_and_ones_that_finish_leave_nothing_behind() {
        let yielding = r#"
            local counter = game:GetService("ReplicatedStorage").Waited
            game:GetService("RunService").Heartbeat:Connect(function()
                task.wait(0.02)
                counter.Value += 1
            end)
        "#;
        let mut g = Game::new(&[("yield.luau", yielding)]);
        let waited = g.number("Waited");
        let s = g.script("Yielder", "yield.luau");
        g.host.start_all();
        g.frames(30);
        let running = g.value(waited);
        assert!(running >= 10.0, "handlers that wait resume and finish, got {running}");

        g.dm.borrow_mut().destroy(s);
        g.frames(2);
        let stopped = g.value(waited);
        g.frames(30);
        assert_eq!(g.value(waited), stopped, "the waiting handlers died with their script");
    }

    #[test]
    fn destroyed_instances_drop_their_signals() {
        let src = "workspace.P.Touched:Connect(function() end) workspace.P:GetPropertyChangedSignal('Size'):Connect(function() end)";
        let mut g = Game::new(&[("sig.luau", src)]);
        let p = g.add("Part", "P", "Workspace", &[]);
        g.script("Sig", "sig.luau");
        g.host.start_all();
        g.frames(2);
        let uid = g.dm.borrow().get(p).unwrap().uid;
        assert!(g.host.ctx.has_signals(uid) && g.host.ctx.existing_signal(uid, "Touched").is_some() && g.host.ctx.existing_signal(uid, "Changed:Size").is_some());
        g.dm.borrow_mut().destroy(p);
        g.frames(2);
        assert!(!g.host.ctx.has_signals(uid), "the part is gone, so are its signals");
        assert!(g.host.ctx.existing_signal(uid, "Touched").is_none());
    }

    #[test]
    fn clear_all_children_tells_scripts_about_every_child() {
        let src = r#"
            local out = game:GetService("ReplicatedStorage").Out
            local folder = workspace.Folder
            local removed, destroying = 0, 0
            folder.ChildRemoved:Connect(function() removed += 1 end)
            for _, c in ipairs(folder:GetChildren()) do
                c.Destroying:Connect(function() destroying += 1 end)
            end
            local before = #folder:GetChildren()
            folder:ClearAllChildren()
            out.Value = before .. ":" .. #folder:GetChildren()
            task.wait()
            task.wait()
            out.Value = out.Value .. ":" .. removed .. ":" .. destroying
        "#;
        let mut g = Game::new(&[("clear.luau", src)]);
        let out = g.add("StringValue", "Out", "ReplicatedStorage", &[]);
        let folder = g.add("Folder", "Folder", "Workspace", &[]);
        for i in 0..25 {
            let mut dm = g.dm.borrow_mut();
            let p = dm.create(if i % 5 == 0 { "Model" } else { "Part" }).unwrap();
            dm.set_parent(p, Some(folder)).unwrap();
        }
        g.script("Clear", "clear.luau");
        g.host.start_all();
        g.frames(5);
        assert_eq!(g.text(out), "25:0:25:25");
    }

    #[test]
    fn a_runaway_script_is_interrupted_and_the_game_goes_on() {
        let mut g = Game::new(&[("spin.luau", "while true do end"), ("frames.luau", COUNT_FRAMES)]);
        let frames = g.number("Frames");
        g.script("Spin", "spin.luau");
        g.script("Frames", "frames.luau");
        g.host.start_all();
        g.frames(5);
        assert!(g.value(frames) >= 3.0, "other scripts keep running after one was interrupted, got {}", g.value(frames));
    }

    #[test]
    fn a_host_can_be_created_and_dropped_repeatedly_over_one_data_model() {
        let dm = Rc::new(RefCell::new(DataModel::new()));
        for _ in 0..3 {
            let host = ScriptHost::new(GameSource::Dir(std::env::temp_dir()), dm.clone()).expect("host");
            host.start_all();
            drop(host);
        }
        assert_eq!(Rc::strong_count(&dm), 1, "dropping the host releases the data model");
    }

    fn collect_garbage(host: &ScriptHost) {
        host.lua.gc_collect().ok();
    }
}
