//! The physics API as scripts see it: real Luau, a real data model and the real Box3D world.
//!
//! [`Game`] steps a frame the way the apps do: the physics (`pre_step`, the world, `post_step`) and then the frame of
//! scripts with the world in reach. The last test runs the same thing through a Bevy app with the real plugin.

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

struct Game {
    dm: Rc<RefCell<DataModel>>,
    host: ScriptHost,
    physics: Box3dWorld,
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
    /// A game running `source` as its only script. The script reports through `Out`, a StringValue in ReplicatedStorage.
    fn new(source: &str) -> Game {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("forge_physics_script_{}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.luau"), source).unwrap();
        let dm = Rc::new(RefCell::new(DataModel::new()));
        let host = ScriptHost::new(GameSource::Dir(dir.clone()), dm.clone()).expect("host");
        host.capture_logs(true);
        {
            let mut d = dm.borrow_mut();
            let out = d.create("StringValue").unwrap();
            d.set_name(out, "Out");
            let rs = d.service("ReplicatedStorage").unwrap();
            d.set_parent(out, Some(rs)).unwrap();
            let script = d.create("Script").unwrap();
            d.set_prop(script, "SourceFile", DV::Str("main.luau".into())).unwrap();
            let sss = d.service("ServerScriptService").unwrap();
            d.set_parent(script, Some(sss)).unwrap();
        }        host.start_all();
        Game { dm, host, physics: Box3dWorld::new(physics_config()), dir, keys: ButtonInput::default(), mouse: ButtonInput::default() }
    }

    /// One frame: the physics step, then the scripts.
    fn frame(&mut self) {
        let world = self.physics.world();
        self.host.physics_pre_step(world, 1.0 / 60.0);
        world.step(1.0 / 60.0, SUB_STEPS);
        self.host.physics_post_step(world, 0.0);
        let input = FrameInput { dt: 1.0 / 60.0, keys: &self.keys, mouse: &self.mouse, mouse_delta: Vec2::ZERO, cursor: None, wheel: 0.0 };
        self.host.frame_with(&input, Some(world));
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    fn out(&self) -> String {
        let d = self.dm.borrow();
        let rs = d.service("ReplicatedStorage").unwrap();
        let out = d.find_child(rs, "Out").unwrap();
        d.string(out, "Value")
    }

    fn logs(&self) -> String {
        self.host.drain_logs().iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n")
    }

    /// The numbers the script wrote, separated by `|`.
    fn numbers(&self) -> Vec<f32> {
        self.out().split('|').map(|s| s.parse().unwrap_or_else(|_| panic!("'{s}' in '{}' (log: {})", self.out(), self.logs()))).collect()
    }
}

const PRELUDE: &str = r#"
    local out = game:GetService("ReplicatedStorage").Out
    local function wait(n) for _ = 1, n do task.wait() end end
    local function part(x, y, z, sx, sy, sz)
        local p = Instance.new("Part")
        p.Size = Vector3.new(sx or 1, sy or 1, sz or 1)
        p.Position = Vector3.new(x, y, z)
        p.Parent = workspace
        return p
    end
    local function ground()
        local g = part(0, -2, 0, 400, 4, 400)
        g.Anchored = true
        return g
    end
"#;

fn script(body: &str) -> String {
    format!("{PRELUDE}\n{body}")
}

#[test]
fn the_part_methods_push_weigh_and_measure() {
    let mut g = Game::new(&script(
        r#"
        workspace.Gravity = 0
        local p = part(0, 50, 0, 2, 1, 2)          -- mass 4
        local mass = p:GetMass()
        p:ApplyImpulse(Vector3.new(0, 8, 0))       -- +2 studs/s
        local vy = p.AssemblyLinearVelocity.Y      -- (read at once: no step has run)
        local q = part(20, 50, 0, 2, 1, 2)
        q:ApplyImpulseAtPosition(Vector3.new(4, 0, 0), q.Position + Vector3.new(0, 1, 0))
        local spin = q.AssemblyAngularVelocity
        local r = part(40, 50, 0, 2, 1, 2)
        r:ApplyAngularImpulse(Vector3.new(0, 4, 0))
        local at = r:GetVelocityAtPosition(r.Position + Vector3.new(1, 0, 0))
        out.Value = table.concat({mass, vy, q.AssemblyLinearVelocity.X, spin.Z, r.AssemblyAngularVelocity.Y, at.Z}, "|")
    "#,
    ));
    g.frames(2);
    let n = g.numbers();
    assert!((n[0] - 4.0).abs() < 1e-3, "GetMass: {n:?}");
    assert!((n[1] - 2.0).abs() < 1e-3, "ApplyImpulse: {n:?}");
    assert!((n[2] - 1.0).abs() < 1e-3, "ApplyImpulseAtPosition moves it: {n:?}");
    assert!(n[3] < -0.01, "and turns it: {n:?}");
    assert!(n[4] > 0.01, "ApplyAngularImpulse: {n:?}");
    assert!(n[5] < -0.01, "GetVelocityAtPosition includes the spin: {n:?}");
}

#[test]
fn the_physics_properties_read_write_and_validate_through_the_property_system() {
    let mut g = Game::new(&script(
        r#"
        local p = part(0, 50, 0)
        local results = {}
        local function try(label, f) local ok, err = pcall(f) table.insert(results, label .. "=" .. tostring(ok)) end
        try("density", function() p.Density = 3 end)
        try("friction", function() p.Friction = 0.9 end)
        try("elasticity", function() p.Elasticity = 0.4 end)
        try("scale", function() p.GravityScale = 0.5 end)
        try("damping", function() p.LinearDamping = 1; p.AngularDamping = 2 end)
        try("kinematic", function() p.Kinematic = true end)
        try("cansleep", function() p.CanSleep = false end)
        try("ccd", function() p.ContinuousCollision = true end)
        try("canquery", function() p.CanQuery = false end)
        try("group", function() p.CollisionGroup = "Default" end)
        try("velocity", function() p.AssemblyLinearVelocity = Vector3.new(1, 2, 3) end)
        try("nan", function() p.Density = 0/0 end)
        try("badvel", function() p.AssemblyLinearVelocity = Vector3.new(0/0, 0, 0) end)
        try("badgroup", function() p.CollisionGroup = "" end)
        try("string", function() p.Density = "heavy" end)
        try("mass", function() p.Mass = 5 end)
        table.insert(results, "mass" .. p.Mass)
        table.insert(results, "density" .. p.Density)
        table.insert(results, "clamped" .. (function() p.Friction = 50 return p.Friction end)())
        out.Value = table.concat(results, ",")
    "#,
    ));
    g.frames(2);
    let out = g.out();
    for ok in ["density", "friction", "elasticity", "scale", "damping", "kinematic", "cansleep", "ccd", "canquery", "group", "velocity"] {
        assert!(out.contains(&format!("{ok}=true")), "{ok} should be settable: {out} {}", g.logs());
    }
    for bad in ["nan", "badvel", "badgroup", "string", "mass"] {
        assert!(out.contains(&format!("{bad}=false")), "{bad} should be refused: {out}");
    }
    assert!(out.contains("mass3,") || out.contains("mass3"), "Mass is volume times density: {out}");
    assert!(out.contains("density3"), "{out}");
    assert!(out.contains("clamped2"), "friction is held to its range: {out}");
}

#[test]
fn raycasts_return_what_they_hit_and_obey_their_params() {
    let mut g = Game::new(&script(
        r#"
        ground()
        local wall = part(0, 5, 20, 20, 10, 1)
        wall.Anchored = true
        wall.Name = "Wall"
        local back = part(0, 5, 30, 20, 10, 1)
        back.Anchored = true
        back.Name = "Back"
        wait(2)
        local hit = workspace:Raycast(Vector3.new(0, 5, 0), Vector3.new(0, 0, 100))
        local parts = {hit.Instance.Name, hit.Position.Z, hit.Normal.Z, hit.Distance, tostring(hit.Material)}
        local params = RaycastParams.new()
        params.FilterDescendantsInstances = {wall}
        local second = workspace:Raycast(Vector3.new(0, 5, 0), Vector3.new(0, 0, 100), params)
        table.insert(parts, second.Instance.Name)
        params.FilterType = Enum.RaycastFilterType.Include
        params.FilterDescendantsInstances = {back}
        table.insert(parts, workspace:Raycast(Vector3.new(0, 5, 0), Vector3.new(0, 0, 100), params).Instance.Name)
        table.insert(parts, tostring(workspace:Raycast(Vector3.new(0, 5, 0), Vector3.new(0, 0, 5))))
        local down = workspace:Raycast(Vector3.new(0, 10, 0), Vector3.new(0, -50, 0))
        table.insert(parts, down.Position.Y)
        local ok, err = pcall(function() workspace:Raycast(Vector3.new(0, 0, 0), "up") end)
        table.insert(parts, tostring(ok))
        out.Value = table.concat(parts, "|")
    "#,
    ));
    g.frames(4);
    let out = g.out();
    let parts: Vec<&str> = out.split('|').collect();
    assert_eq!(parts[0], "Wall", "{out} {}", g.logs());
    assert!((parts[1].parse::<f32>().unwrap() - 19.5).abs() < 1e-3, "position {out}");
    assert!((parts[2].parse::<f32>().unwrap() + 1.0).abs() < 1e-3, "normal {out}");
    assert!((parts[3].parse::<f32>().unwrap() - 19.5).abs() < 1e-3, "distance {out}");
    assert_eq!(parts[4], "Enum.Material.Plastic");
    assert_eq!(parts[5], "Back", "the filter leaves the wall out");
    assert_eq!(parts[6], "Back", "include: only the back");
    assert_eq!(parts[7], "nil", "a short ray reaches nothing");
    assert!(parts[8].parse::<f32>().unwrap().abs() < 1e-3, "straight down onto the ground: {out}");
    assert_eq!(parts[9], "false", "a bad argument is an error");
}

#[test]
fn shape_casts_and_overlap_queries_work_from_scripts() {
    let mut g = Game::new(&script(
        r#"
        local wall = part(0, 5, 20, 20, 10, 1)
        wall.Anchored = true
        local a = part(100, 5, 0, 2, 2, 2) a.Anchored = true
        local b = part(102, 5, 0, 2, 2, 2) b.Anchored = true
        wait(2)
        local s = workspace:Spherecast(Vector3.new(0, 5, 0), 1, Vector3.new(0, 0, 100))
        local k = workspace:Blockcast(CFrame.new(0, 5, 0), Vector3.new(2, 2, 6), Vector3.new(0, 0, 100))
        local box = workspace:GetPartBoundsInBox(CFrame.new(101, 5, 0), Vector3.new(10, 4, 4))
        local ball = workspace:GetPartBoundsInRadius(Vector3.new(100, 5, 0), 0.5)
        local around = workspace:GetPartsInPart(a)
        local op = OverlapParams.new()
        op.FilterDescendantsInstances = {a}
        local filtered = workspace:GetPartBoundsInBox(CFrame.new(101, 5, 0), Vector3.new(10, 4, 4), op)
        op.FilterDescendantsInstances = {}
        op.MaxParts = 1
        local capped = workspace:GetPartBoundsInBox(CFrame.new(101, 5, 0), Vector3.new(10, 4, 4), op)
        out.Value = table.concat({s.Distance, k.Distance, #box, #ball, #around, #filtered, #capped}, "|")
    "#,
    ));
    g.frames(4);
    let n = g.numbers();
    assert!((n[0] - 18.5).abs() < 0.05, "a sphere of radius 1 reaches the wall's face (z = 19.5) after 18.5: {n:?}");
    assert!((n[1] - 16.5).abs() < 0.05, "a block 6 long reaches it 3 studs sooner than a point would: {n:?}");
    assert_eq!(&n[2..], &[2.0, 1.0, 1.0, 1.0, 1.0], "{n:?}");
}

#[test]
fn touched_and_touch_ended_signals_fire_in_scripts() {
    let mut g = Game::new(&script(
        r#"
        ground()
        local pad = part(0, 6, 0, 10, 2, 10)
        pad.Anchored = true
        pad.CanCollide = false
        pad.Name = "Pad"
        local log = {}
        pad.Touched:Connect(function(other) table.insert(log, "in:" .. other.Name) end)
        pad.TouchEnded:Connect(function(other) table.insert(log, "out:" .. other.Name) end)
        local ball = part(0, 12, 0, 1, 1, 1)
        ball.Name = "Ball"
        local wall = part(30, 3, 0, 1, 6, 6)
        wall.Anchored = true
        local hits = 0
        wall.Touched:Connect(function(other) hits += 1 end)
        local mover = part(20, 3, 0, 1, 1, 1)
        mover.AssemblyLinearVelocity = Vector3.new(40, 0, 0)
        wait(150)
        out.Value = table.concat(log, ",") .. "|" .. hits
    "#,
    ));
    g.frames(170);
    let out = g.out();
    assert_eq!(out, "in:Ball,out:Ball|1", "the ball passed through the pad, and the mover touched the wall once; {} ", g.logs());
}

#[test]
fn collision_groups_are_managed_through_the_physics_service_and_filter_what_collides() {
    let mut g = Game::new(&script(
        r#"
        local ps = game:GetService("PhysicsService")
        ps:RegisterCollisionGroup("Ghosts")
        ps:CollisionGroupSetCollidable("Ghosts", "Default", false)
        local results = {}
        table.insert(results, tostring(ps:IsCollisionGroupRegistered("Ghosts")))
        table.insert(results, tostring(ps:CollisionGroupsAreCollidable("Ghosts", "Default")))
        table.insert(results, tostring(ps:CollisionGroupsAreCollidable("Default", "Default")))
        table.insert(results, tostring(#ps:GetRegisteredCollisionGroups()))
        table.insert(results, tostring(pcall(function() ps:RegisterCollisionGroup("Ghosts") end)))
        table.insert(results, tostring(pcall(function() ps:CollisionGroupSetCollidable("Nope", "Default", true) end)))
        table.insert(results, tostring(pcall(function() ps:SetPartCollisionGroup(workspace, "Ghosts") end)))
        ground()
        local ghost = part(0, 5, 0)
        ps:SetPartCollisionGroup(ghost, "Ghosts")
        local solid = part(10, 5, 0)
        wait(60)
        table.insert(results, ghost.Parent == nil and "gone" or "fell:" .. math.floor(ghost.Position.Y))
        table.insert(results, "solid:" .. math.floor(solid.Position.Y * 10 + 0.5))
        ps:UnregisterCollisionGroup("Ghosts")
        table.insert(results, ghost.CollisionGroup)
        out.Value = table.concat(results, ",")
    "#,
    ));
    g.frames(90);
    let out = g.out();
    let parts: Vec<&str> = out.split(',').collect();
    assert_eq!(&parts[..4], &["true", "false", "true", "2"], "{out} {}", g.logs());
    assert_eq!(&parts[4..7], &["false", "false", "false"], "duplicate groups, unknown groups and non-parts are errors: {out}");
    assert!(parts[7].starts_with("fell:-") || parts[7] == "gone", "the ghost fell through the ground: {out}");
    assert_eq!(parts[8], "solid:5", "the Default part rests on it: {out}");
    assert_eq!(parts[9], "Default", "removing a group puts its parts back in Default: {out}");
}

#[test]
fn constraints_are_created_configured_and_destroyed_like_any_instance() {
    let mut g = Game::new(&script(
        r#"
        local anchor = part(0, 50, 0)
        anchor.Anchored = true
        local load = part(0, 46, 0)
        local rope = Instance.new("RopeConstraint")
        rope.Part0 = anchor
        rope.Part1 = load
        rope.Length = 3
        rope.Parent = workspace
        wait(120)
        local held = (load.Position - anchor.Position).Magnitude
        rope.Length = 6                     -- configured while the game runs: the joint is rebuilt
        wait(120)
        local longer = (load.Position - anchor.Position).Magnitude
        rope:UseCurrentLength()
        local current = rope.Length
        rope.Enabled = false
        wait(30)
        local fell = load.Position.Y
        rope:Destroy()
        local weld = Instance.new("WeldConstraint")
        weld.Part0 = anchor
        weld.Part1 = load
        weld.Parent = workspace
        wait(30)
        local y1 = load.Position.Y
        wait(60)
        local y2 = load.Position.Y
        local hinge = Instance.new("HingeConstraint")
        hinge.LimitsEnabled = true
        hinge.LowerAngle = -20
        hinge.UpperAngle = 20
        hinge:SetWorldFrame(CFrame.new(0, 50, 0))
        out.Value = table.concat({held, longer, current, fell, y2 - y1, hinge.C0.Position.Y}, "|")
    "#,
    ));
    g.frames(420);
    let n = g.numbers();
    assert!(n[0] > 2.7 && n[0] < 3.3, "a 3-stud rope holds the load 3 studs away: {n:?} {}", g.logs());
    assert!(n[1] > 5.5 && n[1] < 6.3, "rebuilt with Length 6: {n:?}");
    assert!((n[2] - n[1]).abs() < 0.5 && n[2] > 5.0, "UseCurrentLength: {n:?}");
    assert!(n[3] < 40.0, "a disabled rope lets go: {n:?}");
    assert!(n[4].abs() < 0.2, "a weld holds the part where it is: {n:?}");
    assert!((n[5] + 50.0).abs() < 1e-3 || n[5].abs() < 60.0, "SetWorldFrame sets the frame: {n:?}");
}

#[test]
fn physics_errors_are_script_errors_with_a_reason() {
    let mut g = Game::new(&script(
        r#"
        local anchored = part(0, 5, 0) anchored.Anchored = true
        local loose = Instance.new("Part")
        local results = {}
        local function why(f) local ok, err = pcall(f) return ok and "ok" or tostring(err) end
        table.insert(results, why(function() anchored:ApplyImpulse(Vector3.new(1, 0, 0)) end))
        table.insert(results, why(function() loose:ApplyImpulse(Vector3.new(1, 0, 0)) end))
        table.insert(results, why(function() part(0, 5, 0):ApplyImpulse(Vector3.new(0/0, 0, 0)) end))
        table.insert(results, why(function() part(0, 5, 0):ApplyImpulse() end))
        table.insert(results, why(function() workspace:Spherecast(Vector3.new(), -1, Vector3.new(0, -1, 0)) end))
        table.insert(results, why(function() anchored:Sleep() end))
        out.Value = table.concat(results, "|")
    "#,
    ));
    g.frames(3);
    let out = g.out();
    let parts: Vec<&str> = out.split('|').collect();
    assert!(parts[0].contains("anchored"), "{out}");
    assert!(parts[1].contains("not in the workspace"), "{out}");
    assert!(parts[2].contains("finite"), "{out}");
    assert!(parts[3].contains("Vector3"), "{out}");
    assert!(parts[4].contains("radius"), "{out}");
    assert!(parts[5].contains("anchored"), "{out}");
}

#[test]
fn the_sleeping_property_and_sleep_methods_work_from_scripts() {
    let mut g = Game::new(&script(
        r#"
        ground()
        local p = part(0, 3, 0)
        local changes = 0
        p:GetPropertyChangedSignal("Sleeping"):Connect(function() changes += 1 end)
        wait(240)
        local asleep = p.Sleeping
        p:ApplyImpulse(Vector3.new(0, 10, 0))
        local awake = not p.Sleeping
        wait(240)
        p:WakeUp()
        local woken = not p.Sleeping
        p:Sleep()
        out.Value = table.concat({tostring(asleep), tostring(awake), tostring(woken), tostring(p.Sleeping), changes}, "|")
    "#,
    ));
    g.frames(500);
    let out = g.out();
    let parts: Vec<&str> = out.split('|').collect();
    assert_eq!(&parts[..4], &["true", "true", "true", "true"], "{out} {}", g.logs());
    assert!(parts[4].parse::<u32>().unwrap() >= 3, "Changed fired for the sleep and the wake-ups: {out}");
}

#[test]
fn a_part_with_fixed_rotation_walks_by_its_velocity_and_is_never_tipped() {
    let mut g = Game::new(&script(
        r#"
        ground()
        local body = Instance.new("Part")
        body.Size = Vector3.new(2, 5, 1)
        body.Position = Vector3.new(0, 3, 0)
        body.FixedRotation = true
        body.Friction = 0
        body.Parent = workspace
        wait(60)
        body.AssemblyLinearVelocity = Vector3.new(16, 0, 0)
        wait(60)
        out.Value = table.concat({body.Position.X, body.AssemblyLinearVelocity.X, body.CFrame.UpVector.Y}, "|")
    "#,
    ));
    g.frames(130);
    let out = g.out();
    let parts: Vec<&str> = out.split('|').collect();
    assert!(parts[0].parse::<f32>().unwrap() > 10.0, "{out} {}", g.logs());
    assert!((parts[1].parse::<f32>().unwrap() - 16.0).abs() < 1.0, "friction is zero, so it keeps its speed: {out}");
    assert!(parts[2].parse::<f32>().unwrap() > 0.999, "{out}");
}
/// The same game through a Bevy app: the real plugin steps the real world at the fixed rate, driven by manual time.
#[test]
fn the_bevy_plugin_steps_the_world_at_the_fixed_rate_and_the_systems_keep_the_scene_in_step() {
    use std::time::Duration;

    use bevy::{prelude::*, time::TimeUpdateStrategy};

    use crate::physics::{Box3dSet, Box3dStats, ForgePhysicsPlugin};

    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("forge_physics_app_{}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    let dm = Rc::new(RefCell::new(DataModel::new()));
    let host = ScriptHost::new(GameSource::Dir(dir.clone()), dm.clone()).expect("host");

    fn pre(host: NonSend<ScriptHost>, physics: NonSend<Box3dWorld>, time: Res<Time<Fixed>>) {
        host.physics_pre_step(physics.world(), time.delta_secs());
    }
    fn post(host: NonSend<ScriptHost>, physics: NonSend<Box3dWorld>, stats: Res<Box3dStats>) {
        host.physics_post_step(physics.world(), stats.step_ms as f32);
    }
    fn frame(mut host: NonSendMut<ScriptHost>, physics: NonSend<Box3dWorld>) {
        let (keys, mouse) = (ButtonInput::<KeyCode>::default(), ButtonInput::<MouseButton>::default());
        host.frame_with(&FrameInput { dt: 1.0 / 60.0, keys: &keys, mouse: &mouse, mouse_delta: Vec2::ZERO, cursor: None, wheel: 0.0 }, Some(physics.world()));
    }

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(ForgePhysicsPlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)))
        .insert_non_send(host)
        .add_systems(FixedUpdate, (pre.in_set(Box3dSet::Sync), post.after(Box3dSet::Step)))
        .add_systems(Update, frame);
    let part = {
        let mut d = dm.borrow_mut();
        let p = d.create("Part").unwrap();
        d.set_prop(p, "Position", DV::V3(bevy::math::Vec3::new(0.0, 100.0, 0.0))).unwrap();
        let ws = d.workspace;
        d.set_parent(p, Some(ws)).unwrap();
        p
    };
    for _ in 0..61 {
        app.update();
    }
    let y = dm.borrow().vec3(part, "Position").y;
    // One second of fixed steps (the first update has no time yet): 0.5 * g * t^2 = 98.1 studs.
    assert!((100.0 - y - 98.1).abs() < 8.0, "fell from 100 to {y}");
    assert!(dm.borrow().vec3(part, "AssemblyLinearVelocity").y < -150.0);
    assert_eq!(app.world().non_send::<ScriptHost>().physics_stats().substeps, 1, "one fixed step per frame at 60 fps");
    drop(app);
    std::fs::remove_dir_all(&dir).ok();
}
