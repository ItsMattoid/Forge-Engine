//! Lightweight CPU benchmarks for the Luau `Instance` API, the data model and the render mirror.
//!
//! They are `#[ignore]`d so `cargo test` stays fast. Run them in release mode, one at a time:
//!
//! ```text
//! cargo test --release -p forge_engine --test perf -- --ignored --nocapture --test-threads=1
//! FORGE_BENCH_N=20000 cargo test --release -p forge_engine --test perf -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Every workload prints `scripts+physics` (the host's frame: Luau, events, physics), `sync` (the
//! render mirror: `render::sync` applied to a headless Bevy `World`, so it includes the cost of
//! applying the spawn / insert commands but not of the GPU) and a checksum of the final scene, so a
//! before / after comparison can also show that the results did not change.
//!
//! What this does *not* measure: GPU time, draw calls and Bevy's own render-world extraction. Those
//! need a window; see the notes in `render/part_material.rs` for how the parts are batched.

use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use bevy::{
    ecs::system::SystemState,
    input::{keyboard::KeyCode, mouse::MouseButton, ButtonInput},
    math::{Vec2, Vec3},
    prelude::*,
};
use forge_engine::{
    assets::GameSource,
    core::{Id, Value},
    dm::DataModel,
    render::{self, AssetCache, Mode, PartMaterial, SpawnParams, SyncOptions},
    physics::{physics_config, Box3dWorld, SUB_STEPS},
    scripting::{FrameInput, ScriptHost},
};

fn n() -> usize {
    std::env::var("FORGE_BENCH_N").ok().and_then(|v| v.parse().ok()).unwrap_or(5000)
}

struct Rig {
    dm: Rc<RefCell<DataModel>>,
    host: ScriptHost,
    dir: PathBuf,
    keys: ButtonInput<KeyCode>,
    mouse: ButtonInput<MouseButton>,
    world: World,
    /// The Box3D world the game's physics steps in (stepped by hand, once per frame, as the fixed rate does at 60 fps).
    physics: Box3dWorld,
    source: GameSource,
    frame_time: Duration,
    sync_time: Duration,
    frames: u32,
}

impl Drop for Rig {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

impl Rig {
    fn new(script: &str) -> Rig {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("forge_perf_{}_{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bench.luau"), script).unwrap();
        let dm = Rc::new(RefCell::new(DataModel::new()));
        let source = GameSource::Dir(dir.clone());
        let host = ScriptHost::new(source.clone(), dm.clone()).expect("host");
        host.capture_logs(true);
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<PartMaterial>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<bevy::pbr::decal::ForwardDecalMaterial<bevy::pbr::StandardMaterial>>>();
        world.init_resource::<Assets<bevy::pbr::StandardMaterial>>();
        world.init_resource::<Assets<forge_engine::render::HighlightMaterial>>();
        world.init_resource::<AssetCache>();
        Rig { dm, host, dir, keys: ButtonInput::default(), mouse: ButtonInput::default(), world, physics: Box3dWorld::new(physics_config()), source, frame_time: Duration::ZERO, sync_time: Duration::ZERO, frames: 0 }
    }

    /// Installs the benchmark script as a `Script` in ServerScriptService (it starts with the next frame).
    fn install_script(&self) {
        let mut dm = self.dm.borrow_mut();
        let id = dm.create("Script").unwrap();
        dm.set_name(id, "Bench");
        dm.set_prop(id, "SourceFile", Value::Str("bench.luau".into())).unwrap();
        let sss = dm.service("ServerScriptService").unwrap();
        dm.set_parent(id, Some(sss)).unwrap();
    }

    /// Installs `count` Scripts that all run the benchmark file (as a cloned enemy or a coin does).
    fn install_scripts(&self, count: usize) {
        let mut dm = self.dm.borrow_mut();
        let sss = dm.service("ServerScriptService").unwrap();
        for _ in 0..count {
            let id = dm.create("Script").unwrap();
            dm.set_prop(id, "SourceFile", Value::Str("bench.luau".into())).unwrap();
            dm.set_parent(id, Some(sss)).unwrap();
        }
    }

    fn workspace(&self) -> Id {
        self.dm.borrow().workspace
    }

    fn add_part(&self, pos: Vec3, size: Vec3, anchored: bool) -> Id {
        let mut dm = self.dm.borrow_mut();
        let id = dm.create("Part").unwrap();
        dm.set_prop(id, "Position", Value::V3(pos)).unwrap();
        dm.set_prop(id, "Size", Value::V3(size)).unwrap();
        dm.set_prop(id, "Anchored", Value::Bool(anchored)).unwrap();
        let ws = dm.workspace;
        dm.set_parent(id, Some(ws)).unwrap();
        id
    }

    /// Runs the render mirror once (timed separately from the host's frame).
    fn sync(&mut self) {
        let started = Instant::now();
        let mut state = SystemState::<(Commands, SpawnParams)>::new(&mut self.world);
        {
            let (mut commands, mut spawn) = state.get_mut(&mut self.world).unwrap();
            let mut dm = self.dm.borrow_mut();
            render::sync(&mut dm, &mut commands, &mut spawn, SyncOptions::still(Mode::Game, Msaa::Off), &self.source);
        }
        state.apply(&mut self.world);
        self.sync_time += started.elapsed();
    }

    /// One game frame followed by a render sync.
    fn frame(&mut self) {
        let input = FrameInput { dt: 1.0 / 60.0, keys: &self.keys, mouse: &self.mouse, mouse_delta: Vec2::ZERO, cursor: None, wheel: 0.0 };
        let started = Instant::now();
        let physics = self.physics.world();
        self.host.physics_pre_step(physics, 1.0 / 60.0);
        physics.step(1.0 / 60.0, SUB_STEPS);
        self.host.physics_post_step(physics, 0.0);
        self.host.frame_with(&input, Some(physics));
        self.frame_time += started.elapsed();
        self.sync();
        self.frames += 1;
    }

    fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.frame();
        }
    }

    fn reset_timers(&mut self) {
        self.frame_time = Duration::ZERO;
        self.sync_time = Duration::ZERO;
        self.frames = 0;
    }

    /// The sum of every part's position: a cheap fingerprint of where everything ended up.
    fn checksum(&self) -> f64 {
        let dm = self.dm.borrow();
        dm.with_component("PrimitiveRenderer").into_iter().map(|id| dm.vec3(id, "Position")).map(|p| p.x as f64 * 1.0 + p.y as f64 * 3.0 + p.z as f64 * 7.0).sum()
    }

    fn entities(&self) -> usize {
        self.world.iter_entities().count()
    }

    fn report(&self, name: &str, ops: usize) {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        // A script that failed would make the numbers meaningless: say so.
        for line in self.host.drain_logs() {
            println!("   [script {:?}] {}", line.level, line.text);
        }
        let total = ms(self.frame_time) + ms(self.sync_time);
        println!(
            "{name:<34} scripts+physics {:>9.2} ms | sync {:>9.2} ms | total {:>9.2} ms | {:>8.3} us/op | frames {:>4} | parts {:>6} | entities {:>6} | checksum {:.3}",
            ms(self.frame_time),
            ms(self.sync_time),
            total,
            total * 1000.0 / ops.max(1) as f64,
            self.frames,
            self.dm.borrow().with_component("PrimitiveRenderer").len(),
            self.entities(),
            self.checksum(),
        );
    }
}

fn part_grid(rig: &Rig, count: usize, anchored: bool) -> Vec<Id> {
    (0..count).map(|i| rig.add_part(Vec3::new((i % 100) as f32 * 3.0, 0.5, (i / 100) as f32 * 3.0), Vec3::new(2.0, 1.0, 2.0), anchored)).collect()
}

// ---- creating objects ------------------------------------------------------------------------------

#[test]
#[ignore]
fn luau_instance_new_only() {
    let count = n();
    let mut rig = Rig::new(&format!(
        r#"
        local N = {count}
        local i = 0
        while i < N do
            for _ = 1, 1000 do
                i += 1
                local p = Instance.new("Part")
            end
            task.wait()
        end
    "#
    ));
    rig.install_script();
    rig.host.start_all();
    rig.frames(count / 1000 + 3);
    rig.report("Instance.new('Part') x N", count);
}

#[test]
#[ignore]
fn luau_instance_new_props_parent() {
    let count = n();
    let mut rig = Rig::new(&format!(
        r#"
        local N = {count}
        local ws = workspace
        local i = 0
        while i < N do
            for _ = 1, 1000 do
                i += 1
                local p = Instance.new("Part")
                p.Size = Vector3.new(2, 1, 2)
                p.Position = Vector3.new(i % 100, 0.5, i // 100)
                p.Color = Color3.new(0.2, 0.4, 0.8)
                p.Anchored = true
                p.Material = Enum.Material.Metal
                p.Parent = ws
            end
            task.wait()
        end
    "#
    ));
    rig.install_script();
    rig.host.start_all();
    rig.frames(count / 1000 + 3);
    rig.report("new + 5 props + Parent x N", count);
}

#[test]
#[ignore]
fn luau_instance_new_with_parent_argument() {
    let count = n();
    let mut rig = Rig::new(&format!(
        r#"
        local N = {count}
        local i = 0
        while i < N do
            for _ = 1, 1000 do
                i += 1
                local p = Instance.new("Part", workspace)
                p.Position = Vector3.new(i % 100, 0.5, i // 100)
            end
            task.wait()
        end
    "#
    ));
    rig.install_script();
    rig.host.start_all();
    rig.frames(count / 1000 + 3);
    rig.report("new(Part, workspace) + Position x N", count);
}

// ---- reading, writing, moving --------------------------------------------------------------------------

#[test]
#[ignore]
fn luau_read_properties() {
    let count = n();
    let mut rig = Rig::new(
        r#"
        local parts = {} for _, c in ipairs(workspace:GetChildren()) do if c.ClassName == "Part" then table.insert(parts, c) end end
        local sum = 0
        for rep = 1, 5 do
            for _, p in ipairs(parts) do
                sum += p.Size.X + p.Position.Y + p.Color.R + #p.Name + (p.Anchored and 1 or 0)
            end
            task.wait()
        end
    "#,
    );
    part_grid(&rig, count, true);
    rig.install_script();
    rig.host.start_all();
    rig.reset_timers();
    rig.frames(8);
    rig.report("read 5 props x N x 5 reps", count * 5 * 5);
}

#[test]
#[ignore]
fn luau_write_unchanged_values() {
    let count = n();
    let mut rig = Rig::new(
        r#"
        local parts = {} for _, c in ipairs(workspace:GetChildren()) do if c.ClassName == "Part" then table.insert(parts, c) end end
        for rep = 1, 5 do
            for _, p in ipairs(parts) do
                p.Position = p.Position
                p.Color = p.Color
                p.Anchored = p.Anchored
            end
            task.wait()
        end
    "#,
    );
    part_grid(&rig, count, true);
    rig.sync(); // spawn the entities first, so only the rewrites are measured
    rig.install_script();
    rig.host.start_all();
    rig.reset_timers();
    rig.frames(8);
    rig.report("rewrite unchanged x3 props x N x 5", count * 3 * 5);
}

fn move_script(moving: usize) -> String {
    format!(
        r#"
        local parts = {{}} for _, c in ipairs(workspace:GetChildren()) do if c.ClassName == "Part" then table.insert(parts, c) end end
        local moving = math.min({moving}, #parts)
        game:GetService("RunService").Heartbeat:Connect(function(dt)
            local up = Vector3.new(0, 0.01, 0)
            for i = 1, moving do
                local p = parts[i]
                p.Position = p.Position + up
            end
        end)
    "#
    )
}

#[test]
#[ignore]
fn luau_move_all_parts_anchored() {
    let count = n();
    let mut rig = Rig::new(&move_script(count));
    part_grid(&rig, count, true);
    rig.install_script();
    rig.host.start_all();
    rig.frames(3); // spawn entities, start the script
    rig.reset_timers();
    rig.frames(60);
    rig.report("move all N anchored, 60 frames", count * 60);
}

#[test]
#[ignore]
fn luau_move_50_of_n_anchored() {
    let count = n();
    let mut rig = Rig::new(&move_script(50));
    part_grid(&rig, count, true);
    rig.install_script();
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(120);
    rig.report("move 50 of N anchored, 120 frames", 50 * 120);
}

#[test]
#[ignore]
fn luau_move_all_parts_unanchored_cannot_collide() {
    // Not anchored but not colliding: only the transform path, no static-world rebuilds.
    let count = n();
    let mut rig = Rig::new(&move_script(count));
    let ids = part_grid(&rig, count, true);
    for id in &ids {
        rig.dm.borrow_mut().set_prop(*id, "CanCollide", Value::Bool(false)).unwrap();
    }
    rig.install_script();
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(60);
    rig.report("move all N, CanCollide=false", count * 60);
}

// ---- structure -------------------------------------------------------------------------------------

#[test]
#[ignore]
fn luau_reparent_and_destroy() {
    let count = n();
    let mut rig = Rig::new(
        r#"
        local parts = {} for _, c in ipairs(workspace:GetChildren()) do if c.ClassName == "Part" then table.insert(parts, c) end end
        local folder = Instance.new("Folder")
        folder.Parent = workspace
        for _, p in ipairs(parts) do p.Parent = folder end
        task.wait()
        for _, p in ipairs(parts) do p.Parent = workspace end
        task.wait()
        for _, p in ipairs(parts) do p:Destroy() end
        task.wait()
    "#,
    );
    part_grid(&rig, count, true);
    rig.sync();
    rig.install_script();
    rig.host.start_all();
    rig.reset_timers();
    rig.frames(6);
    rig.report("reparent x2 + Destroy x N", count * 3);
}

#[test]
#[ignore]
fn luau_clear_all_children() {
    let count = n();
    let mut rig = Rig::new("task.wait() workspace:ClearAllChildren() task.wait()");
    part_grid(&rig, count, true);
    rig.sync();
    rig.install_script();
    rig.host.start_all();
    rig.reset_timers();
    rig.frames(4);
    rig.report("ClearAllChildren of N", count);
}

#[test]
#[ignore]
fn luau_clone_model() {
    let count = n().min(2000);
    let mut rig = Rig::new(
        r#"
        local model = workspace.Model
        for i = 1, 5 do
            local c = model:Clone()
            c.Parent = workspace
        end
    "#,
    );
    {
        let model = rig.dm.borrow_mut().create("Model").unwrap();
        for i in 0..count {
            let p = rig.dm.borrow_mut().create("Part").unwrap();
            rig.dm.borrow_mut().set_prop(p, "Position", Value::V3(Vec3::new(i as f32, 0.0, 0.0))).unwrap();
            rig.dm.borrow_mut().set_parent(p, Some(model)).unwrap();
        }
        rig.dm.borrow_mut().set_name(model, "Model");
        let ws = rig.workspace();
        rig.dm.borrow_mut().set_parent(model, Some(ws)).unwrap();
    }
    rig.install_script();
    rig.host.start_all();
    rig.frames(1);
    rig.reset_timers();
    rig.frames(3);
    rig.report("Clone a model of N parts x5", count * 5);
}

#[test]
#[ignore]
fn luau_changed_signals() {
    let count = n().min(2000);
    let mut rig = Rig::new(
        r#"
        local hits = 0
        local parts = {} for _, c in ipairs(workspace:GetChildren()) do if c.ClassName == "Part" then table.insert(parts, c) end end
        for _, p in ipairs(parts) do
            p:GetPropertyChangedSignal("Position"):Connect(function() hits += 1 end)
        end
        game:GetService("RunService").Heartbeat:Connect(function()
            for _, p in ipairs(parts) do p.Position = p.Position + Vector3.new(0, 0.01, 0) end
        end)
    "#,
    );
    part_grid(&rig, count, true);
    rig.install_script();
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(30);
    rig.report("Changed signal on N parts, 30 frames", count * 30);
}

// ---- many scripts --------------------------------------------------------------------------------------

#[test]
#[ignore]
fn luau_start_many_scripts_sharing_a_file() {
    let count = n();
    // About the size and shape of a small enemy or pickup script.
    let mut rig = Rig::new(
        r#"
        local RunService = game:GetService("RunService")
        local part = script.Parent
        local CONFIG = { Speed = 8, Range = 40, Damage = 10, Cooldown = 1.5 }
        local state = { target = nil, cooldown = 0, health = 100, alive = true }

        local function distance(a, b)
            local d = a - b
            return math.sqrt(d.X * d.X + d.Y * d.Y + d.Z * d.Z)
        end

        local function pickTarget(origin)
            local best, bestDistance = nil, CONFIG.Range
            for _, candidate in ipairs(workspace:GetChildren()) do
                if candidate.ClassName == "Model" then
                    local d = distance(origin, candidate:GetPivot().Position)
                    if d < bestDistance then
                        best, bestDistance = candidate, d
                    end
                end
            end
            return best
        end

        local function takeDamage(amount)
            state.health -= amount
            if state.health <= 0 and state.alive then
                state.alive = false
            end
        end

        local function update(dt)
            if not state.alive then return end
            state.cooldown = math.max(0, state.cooldown - dt)
            if state.cooldown == 0 then
                state.target = pickTarget(Vector3.new(0, 0, 0))
                state.cooldown = CONFIG.Cooldown
            end
        end

        RunService.Heartbeat:Connect(update)
        script.Destroying:Connect(function() state.alive = false end)
        local x = takeDamage
    "#,
    );
    rig.install_scripts(count);
    rig.host.start_all();
    rig.reset_timers();
    rig.frames(2);
    rig.report("start N scripts of one file", count);
}

#[test]
#[ignore]
fn luau_many_sleeping_threads() {
    // Every script sleeps two seconds at a time: most frames nobody is due, but all of them are waiting.
    let count = n();
    let mut rig = Rig::new("while true do task.wait(2) end");
    rig.install_scripts(count);
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(100);
    rig.report("N threads in task.wait(2), 100 frames", count * 100);
}

#[test]
#[ignore]
fn luau_many_threads_waking_every_frame() {
    let count = n();
    let mut rig = Rig::new("local n = 0 while true do n += 1 task.wait() end");
    rig.install_scripts(count);
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(30);
    rig.report("N threads in task.wait(), 30 frames", count * 30);
}

#[test]
#[ignore]
fn luau_many_heartbeat_handlers() {
    let count = n();
    let mut rig = Rig::new("local n = 0 game:GetService('RunService').Heartbeat:Connect(function() n += 1 end)");
    rig.install_scripts(count);
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(30);
    rig.report("N Heartbeat handlers, 30 frames", count * 30);
}

#[test]
#[ignore]
fn luau_many_scripts_with_the_profiler_on() {
    // What the Script Performance overlay costs while it is open.
    let count = n();
    let mut rig = Rig::new("local n = 0 game:GetService('RunService').Heartbeat:Connect(function() n += 1 end)");
    rig.install_scripts(count);
    rig.host.set_profiling(true);
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(30);
    rig.report("N handlers, profiler on, 30 frames", count * 30);
}

#[test]
#[ignore]
fn luau_changed_signal_handlers_fire() {
    // The handler-per-event path: each event runs a handler in a thread of its own.
    let count = n().min(2000);
    let mut rig = Rig::new(
        r#"
        local hits = 0
        for _, c in ipairs(workspace:GetChildren()) do
            if c.ClassName == "Part" then c.Changed:Connect(function() hits += 1 end) end
        end
        game:GetService("RunService").Heartbeat:Connect(function()
            for _, c in ipairs(workspace:GetChildren()) do
                if c.ClassName == "Part" then c.Transparency = (c.Transparency + 0.01) % 0.5 end
            end
        end)
    "#,
    );
    part_grid(&rig, count, true);
    rig.install_script();
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(30);
    rig.report("Changed handler per part x30 frames", count * 30);
}

// ---- physics -------------------------------------------------------------------------------------

#[test]
#[ignore]
fn physics_touch_sensors_and_movers() {
    // Pads that scripts listen to (coins, kill bricks, checkpoints) and parts that fall over them.
    let count = n().min(3000);
    let mut rig = Rig::new(
        r#"
        for _, c in ipairs(workspace:GetChildren()) do
            if c.Name == "Pad" then c.Touched:Connect(function() end) end
        end
    "#,
    );
    rig.add_part(Vec3::new(150.0, -0.5, 150.0), Vec3::new(400.0, 1.0, 400.0), true);
    for i in 0..count {
        let pad = rig.add_part(Vec3::new((i % 50) as f32 * 4.0, 1.0, (i / 50) as f32 * 4.0), Vec3::new(3.0, 1.0, 3.0), true);
        rig.dm.borrow_mut().set_prop(pad, "CanCollide", Value::Bool(false)).unwrap();
        rig.dm.borrow_mut().set_name(pad, "Pad");
    }
    for i in 0..count / 3 {
        rig.add_part(Vec3::new((i % 40) as f32 * 5.0, 20.0 + (i % 5) as f32 * 3.0, (i / 40) as f32 * 5.0), Vec3::new(2.0, 2.0, 2.0), false);
    }
    rig.install_script();
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(120);
    rig.report("N Touched pads, N/3 falling parts, 120 frames", count * 120);
}

#[test]
#[ignore]
fn physics_falling_parts() {
    let count = n().min(3000);
    let mut rig = Rig::new("");
    rig.add_part(Vec3::new(150.0, -0.5, 150.0), Vec3::new(400.0, 1.0, 400.0), true);
    for i in 0..count {
        rig.add_part(Vec3::new((i % 50) as f32 * 4.0, 20.0 + (i % 7) as f32 * 3.0, (i / 50) as f32 * 4.0), Vec3::new(2.0, 2.0, 2.0), false);
    }
    rig.frames(2);
    rig.reset_timers();
    rig.frames(180);
    rig.report("N unanchored parts falling, 180 frames", count * 180);
}

#[test]
#[ignore]
fn physics_resting_parts_cost_nothing() {
    // Parts that have come to rest are asleep: a scene of them costs (almost) nothing per frame.
    let count = n().min(3000);
    let mut rig = Rig::new("");
    rig.add_part(Vec3::new(150.0, -0.5, 150.0), Vec3::new(400.0, 1.0, 400.0), true);
    for i in 0..count {
        rig.add_part(Vec3::new((i % 50) as f32 * 4.0, 1.5, (i / 50) as f32 * 4.0), Vec3::new(2.0, 2.0, 2.0), false);
    }
    rig.frames(240); // (they settle and go to sleep)
    rig.reset_timers();
    rig.frames(180);
    rig.report("N parts at rest (asleep), 180 frames", count * 180);
}

#[test]
#[ignore]
fn physics_a_pile_of_parts_colliding() {
    // The expensive case: many parts that touch each other and keep moving.
    let count = n().min(2000);
    let mut rig = Rig::new("");
    rig.add_part(Vec3::new(0.0, -0.5, 0.0), Vec3::new(400.0, 1.0, 400.0), true);
    for i in 0..count {
        rig.add_part(Vec3::new((i % 10) as f32 * 2.2, 1.0 + (i / 100) as f32 * 2.2, ((i / 10) % 10) as f32 * 2.2), Vec3::new(2.0, 2.0, 2.0), false);
    }
    rig.frames(2);
    rig.reset_timers();
    rig.frames(120);
    rig.report("N stacked parts settling, 120 frames", count * 120);
}

#[test]
#[ignore]
fn physics_raycasts_from_scripts() {
    // A script casting rays every frame (ground checks, line of sight, shooting).
    let rays = 500;
    let mut rig = Rig::new(&format!(
        r#"
        local hits = 0
        game:GetService("RunService").Heartbeat:Connect(function()
            for i = 1, {rays} do
                if workspace:Raycast(Vector3.new(i % 50 * 4, 30, i // 50 * 4), Vector3.new(0, -100, 0)) then hits += 1 end
            end
        end)
    "#
    ));
    rig.add_part(Vec3::new(100.0, -0.5, 20.0), Vec3::new(400.0, 1.0, 400.0), true);
    for i in 0..n().min(2000) {
        rig.add_part(Vec3::new((i % 50) as f32 * 4.0, 1.0, (i / 50) as f32 * 4.0), Vec3::new(2.0, 2.0, 2.0), true);
    }
    rig.install_script();
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(60);
    rig.report("500 Raycasts per frame, 60 frames", rays * 60);
}

#[test]
#[ignore]
fn physics_impulses_and_velocity_reads_from_scripts() {
    let count = n().min(2000);
    let mut rig = Rig::new(
        r#"
        local parts = {}
        for _, c in ipairs(workspace:GetChildren()) do
            if c.ClassName == "Part" and not c.Anchored then table.insert(parts, c) end
        end
        game:GetService("RunService").Heartbeat:Connect(function()
            for _, p in ipairs(parts) do
                p:ApplyImpulse(Vector3.new(0, 0.5, 0))
                local _ = p.AssemblyLinearVelocity.Y
            end
        end)
    "#,
    );
    rig.add_part(Vec3::new(150.0, -0.5, 150.0), Vec3::new(400.0, 1.0, 400.0), true);
    for i in 0..count {
        rig.add_part(Vec3::new((i % 50) as f32 * 4.0, 20.0 + (i % 7) as f32 * 3.0, (i / 50) as f32 * 4.0), Vec3::new(2.0, 2.0, 2.0), false);
    }
    rig.install_script();
    rig.host.start_all();
    rig.frames(3);
    rig.reset_timers();
    rig.frames(60);
    rig.report("ApplyImpulse + velocity read per part, 60 frames", count * 60);
}

#[test]
#[ignore]
fn physics_a_chain_of_constraints() {
    let links = 200;
    let mut rig = Rig::new("");
    let mut previous = rig.add_part(Vec3::new(0.0, 100.0, 0.0), Vec3::ONE, true);
    {
        for i in 1..=links {
            let part = rig.add_part(Vec3::new(i as f32 * 1.5, 100.0, 0.0), Vec3::ONE, false);
            let mut dm = rig.dm.borrow_mut();
            let rope = dm.create("RodConstraint").unwrap();
            dm.set_prop(rope, "Part0", Value::Ref(Some(previous))).unwrap();
            dm.set_prop(rope, "Part1", Value::Ref(Some(part))).unwrap();
            dm.set_prop(rope, "Length", Value::Num(1.5)).unwrap();
            let ws = dm.workspace;
            dm.set_parent(rope, Some(ws)).unwrap();
            previous = part;
        }
    }
    rig.frames(2);
    rig.reset_timers();
    rig.frames(120);
    rig.report("a chain of 200 rods swinging, 120 frames", links * 120);
}

// ---- the render mirror ---------------------------------------------------------------------------------

#[test]
#[ignore]
fn render_spawn_recolor_move() {
    let count = n();
    let mut rig = Rig::new("");
    part_grid(&rig, count, true);
    rig.reset_timers();
    rig.sync();
    rig.report("render: first sync (spawn N)", count);

    rig.reset_timers();
    for round in 0..10 {
        let mut dm = rig.dm.borrow_mut();
        for id in dm.with_component("PrimitiveRenderer") {
            dm.set_prop(id, "Color", Value::Color(Vec3::new(round as f32 / 10.0, 0.5, 0.5))).unwrap();
        }
        drop(dm);
        rig.sync();
    }
    rig.report("render: recolor all N x10", count * 10);

    rig.reset_timers();
    for round in 0..10 {
        let mut dm = rig.dm.borrow_mut();
        for id in dm.with_component("PrimitiveRenderer") {
            dm.set_prop(id, "Position", Value::V3(Vec3::new(round as f32, 1.0, id.slot as f32))).unwrap();
        }
        drop(dm);
        rig.sync();
    }
    rig.report("render: move all N x10", count * 10);

    rig.reset_timers();
    for round in 0..100 {
        let mut dm = rig.dm.borrow_mut();
        for id in dm.with_component("PrimitiveRenderer").into_iter().take(50) {
            dm.set_prop(id, "Position", Value::V3(Vec3::new(round as f32, 2.0, id.slot as f32))).unwrap();
        }
        drop(dm);
        rig.sync();
    }
    rig.report("render: move 50 of N x100", 50 * 100);
}

// ---- the data model alone (no Luau) ------------------------------------------------------------------------

#[test]
#[ignore]
fn datamodel_create_set_destroy() {
    let count = n();
    let mut dm = DataModel::new();
    let ws = dm.workspace;

    let started = Instant::now();
    let ids: Vec<Id> = (0..count).map(|_| dm.create("Part").unwrap()).collect();
    let create = started.elapsed();

    let started = Instant::now();
    for (i, id) in ids.iter().enumerate() {
        dm.set_prop(*id, "Position", Value::V3(Vec3::new(i as f32, 1.0, 0.0))).unwrap();
        dm.set_prop(*id, "Color", Value::Color(Vec3::new(0.1, 0.2, 0.3))).unwrap();
        dm.set_prop(*id, "Anchored", Value::Bool(true)).unwrap();
    }
    let props = started.elapsed();

    let started = Instant::now();
    for id in &ids {
        dm.set_parent(*id, Some(ws)).unwrap();
    }
    let parent = started.elapsed();

    let started = Instant::now();
    for _ in 0..10 {
        for (i, id) in ids.iter().enumerate() {
            dm.set_prop(*id, "Position", Value::V3(Vec3::new(i as f32, 2.0, 0.0))).unwrap();
        }
        // what the render mirror does each frame: take the dirty list and clear the flags
        for id in std::mem::take(&mut dm.dirty) {
            if let Some(i) = dm.get_mut(id) {
                i.dirty = 0;
            }
        }
    }
    let moves = started.elapsed();

    let started = Instant::now();
    for id in &ids {
        dm.destroy(*id);
    }
    let destroy = started.elapsed();

    let us = |d: Duration, ops: usize| d.as_secs_f64() * 1e6 / ops as f64;
    println!(
        "datamodel x{count}: create {:.3} us | 3 prop writes {:.3} us | set_parent {:.3} us | move (1 write) {:.3} us | destroy {:.3} us",
        us(create, count),
        us(props, count),
        us(parent, count),
        us(moves, count * 10),
        us(destroy, count)
    );
}
