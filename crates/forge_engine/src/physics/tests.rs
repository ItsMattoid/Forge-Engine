//! The physics, tested against the real Box3D world: parts fall, rest, collide, are pushed, joined and found.
//!
//! The tests drive the bridge the way the apps do (`pre_step`, the world's step, `post_step`) without a Bevy app,
//! so they are fast and can look at everything. [`Sim`] is that harness. Tests of the Luau API are in
//! `scripting::physics_tests`; the Bevy plugin is tested at the bottom.

use bevy::math::{Quat, Vec3};

use super::*;
use crate::{
    core::{Id, Value},
    dm::{DataModel, Event},
};

const DT: f32 = 1.0 / 60.0;

/// A data model, a bridge and a world, stepped by hand.
pub(crate) struct Sim {
    pub dm: DataModel,
    pub bridge: PhysicsBridge,
    pub physics: Box3dWorld,
}

impl Sim {
    pub fn new() -> Sim {
        let mut dm = DataModel::new();
        let mut bridge = PhysicsBridge::new();
        bridge.attach(&mut dm);
        Sim { dm, bridge, physics: Box3dWorld::new(physics_config()) }
    }

    /// One fixed step, as `FixedUpdate` runs it.
    pub fn step(&mut self) {
        let world = self.physics.world();
        self.bridge.pre_step(&mut self.dm, world, DT);
        world.step(DT, SUB_STEPS);
        self.bridge.post_step(&mut self.dm, world);
    }

    pub fn run(&mut self, steps: usize) {
        for _ in 0..steps {
            self.step();
        }
    }

    /// Runs until `done` says so (or `max` steps), returning how many steps it took.
    pub fn run_until(&mut self, max: usize, mut done: impl FnMut(&mut Sim) -> bool) -> usize {
        for n in 0..max {
            if done(self) {
                return n;
            }
            self.step();
        }
        max
    }

    pub fn part(&mut self, pos: Vec3, size: Vec3) -> Id {
        let id = self.dm.create("Part").unwrap();
        self.dm.set_prop(id, "Size", Value::V3(size)).unwrap();
        self.dm.set_prop(id, "Position", Value::V3(pos)).unwrap();
        self.dm.set_parent(id, Some(self.dm.workspace)).unwrap();
        id
    }

    pub fn anchored(&mut self, pos: Vec3, size: Vec3) -> Id {
        let id = self.part(pos, size);
        self.dm.set_prop(id, "Anchored", Value::Bool(true)).unwrap();
        id
    }

    /// A big anchored floor whose top is at y = 0.
    pub fn ground(&mut self) -> Id {
        self.anchored(Vec3::new(0.0, -2.0, 0.0), Vec3::new(400.0, 4.0, 400.0))
    }

    pub fn set(&mut self, id: Id, prop: &str, v: Value) {
        self.dm.set_prop(id, prop, v).unwrap_or_else(|e| panic!("{prop}: {e}"));
    }

    pub fn flag(&mut self, id: Id, prop: &str, on: bool) {
        self.set(id, prop, Value::Bool(on));
    }

    pub fn pos(&self, id: Id) -> Vec3 {
        self.dm.vec3(id, "Position")
    }

    pub fn vel(&self, id: Id) -> Vec3 {
        self.dm.vec3(id, "AssemblyLinearVelocity")
    }

    pub fn sleeping(&self, id: Id) -> bool {
        self.dm.flag(id, "Sleeping")
    }

    /// Runs `f` with the world reachable from the bridge, as it is while a frame of scripts runs.
    pub fn in_frame<R>(&mut self, f: impl FnOnce(&mut PhysicsBridge, &mut DataModel) -> R) -> R {
        let _scope = WorldScope::enter(self.bridge.slot(), self.physics.world());
        f(&mut self.bridge, &mut self.dm)
    }

    pub fn stats(&self) -> PhysicsStats {
        self.bridge.stats.clone()
    }

    pub fn bodies(&self) -> usize {
        self.bridge.body_count()
    }
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

// ---- rigid bodies -----------------------------------------------------------------------------------

#[test]
fn an_unanchored_part_falls_and_comes_to_rest_on_an_anchored_one_and_goes_to_sleep() {
    let mut sim = Sim::new();
    sim.ground();
    let part = sim.part(v(0.0, 10.0, 0.0), v(2.0, 1.0, 2.0));
    sim.run(5);
    assert!(sim.pos(part).y < 10.0 && sim.vel(part).y < 0.0, "it is falling: {:?} {:?}", sim.pos(part), sim.vel(part));
    sim.run_until(600, |s| s.sleeping(part));
    assert!(sim.sleeping(part), "it came to rest and was put to sleep");
    let y = sim.pos(part).y;
    assert!(near(y, 0.5, 0.03), "resting on the floor's top (y = 0): its centre is half its height up, got {y}");
    assert!(sim.vel(part).length() < 0.01, "{:?}", sim.vel(part));
    // It stays where it is, asleep, however long it runs.
    sim.run(120);
    assert_eq!(sim.pos(part).y, y);
    assert_eq!(sim.stats().bodies[0].2, 0, "nothing is awake: a resting scene costs nothing");
}

#[test]
fn an_anchored_part_never_moves_and_nothing_is_written_back_for_it() {
    let mut sim = Sim::new();
    let part = sim.anchored(v(3.0, 40.0, 0.0), v(4.0, 1.0, 2.0));
    sim.run(1);
    sim.dm.get_mut(part).unwrap().dirty = 0; // (as the renderer leaves it)
    sim.run(120);
    assert_eq!(sim.pos(part), v(3.0, 40.0, 0.0));
    assert_eq!(sim.vel(part), Vec3::ZERO);
    assert_eq!(sim.dm.get(part).unwrap().dirty, 0, "the renderer is not told about a part that did not move");
}

#[test]
fn the_world_has_a_body_for_every_simulated_part_and_none_for_the_rest() {
    let mut sim = Sim::new();
    let ws = sim.dm.workspace;
    let a = sim.part(v(0.0, 5.0, 0.0), Vec3::ONE);
    let loose = sim.dm.create("Part").unwrap(); // not in the world
    let light = sim.dm.create("PointLight").unwrap();
    sim.dm.set_parent(light, Some(a)).unwrap();
    let folder = sim.dm.create("Folder").unwrap();
    sim.dm.set_parent(folder, Some(ws)).unwrap();
    let inside = sim.dm.create("Part").unwrap();
    sim.dm.set_parent(inside, Some(folder)).unwrap();
    sim.run(2);
    assert_eq!(sim.bodies(), 2, "the part and the part in the folder (a light, a folder and a loose part are not bodies)");
    assert!(sim.bridge.part_of(sim.bridge.shape_of(a).unwrap()) == Some(a));
    let _ = loose;
}

#[test]
fn gravity_is_the_workspace_property_and_changes_while_running() {
    let mut sim = Sim::new();
    let part = sim.part(v(0.0, 1000.0, 0.0), Vec3::ONE);
    sim.run(30);
    let fallen = 1000.0 - sim.pos(part).y;
    // 0.5 * g * t^2 with g = 196.2 and t = 0.5 s
    assert!(near(fallen, 0.5 * 196.2 * 0.25, 3.0), "fell {fallen}");
    let ws = sim.dm.workspace;
    sim.set(ws, "Gravity", Value::Num(0.0));
    let v0 = sim.vel(part).y;
    sim.run(30);
    assert!(near(sim.vel(part).y, v0, 0.5), "with no gravity the speed stays: {v0} -> {}", sim.vel(part).y);
    sim.set(ws, "Gravity", Value::Num(98.1));
    sim.run(30);
    assert!(sim.vel(part).y > v0 - 98.1 * 0.5 - 2.0 && sim.vel(part).y < v0 - 98.1 * 0.5 + 2.0, "half gravity for half a second: {v0} -> {}", sim.vel(part).y);
}

#[test]
fn gravity_scale_damping_density_and_friction_change_what_a_part_does() {
    let mut sim = Sim::new();
    let heavy = sim.part(v(-5.0, 100.0, 0.0), Vec3::ONE);
    let floaty = sim.part(v(0.0, 100.0, 0.0), Vec3::ONE);
    let damped = sim.part(v(5.0, 100.0, 0.0), Vec3::ONE);
    sim.set(floaty, "GravityScale", Value::Num(0.0));
    sim.set(damped, "LinearDamping", Value::Num(5.0));
    sim.run(60);
    assert!(sim.pos(heavy).y < 90.0);
    assert_eq!(sim.pos(floaty).y, 100.0, "no gravity scale: it hangs there");
    assert!(sim.vel(damped).y > sim.vel(heavy).y, "drag slows the fall: {} vs {}", sim.vel(damped).y, sim.vel(heavy).y);
    // Back to normal while running.
    sim.set(floaty, "GravityScale", Value::Num(1.0));
    sim.run(30);
    assert!(sim.pos(floaty).y < 100.0 - 1.0, "released: it falls");
    sim.set(floaty, "GravityScale", Value::Num(-1.0));
    sim.run(120);
    assert!(sim.vel(floaty).y > 0.0, "a negative scale makes it rise");

    // Density is mass per volume and is applied to the body.
    let block = sim.part(v(0.0, 500.0, 0.0), v(2.0, 3.0, 4.0));
    sim.run(2);
    assert!(near(sim.in_frame(|b, dm| b.mass(dm, block)), 24.0, 1e-3));
    sim.set(block, "Density", Value::Num(2.5));
    sim.run(2);
    assert!(near(sim.in_frame(|b, dm| b.mass(dm, block)), 60.0, 1e-3), "density changed while running");
    assert!(near(sim.dm.num(block, "Mass", 0.0), 60.0, 1e-3), "and the Mass property says so");
}

#[test]
fn elasticity_makes_a_part_bounce_and_friction_makes_it_grip() {
    let mut sim = Sim::new();
    sim.ground();
    let ball = sim.part(v(0.0, 20.0, 0.0), Vec3::splat(2.0));
    sim.set(ball, "Shape", Value::Enum("Ball".into()));
    sim.set(ball, "Elasticity", Value::Num(0.8));
    let mut peak_after_bounce = 0.0f32;
    let mut bounced = false;
    for _ in 0..240 {
        sim.step();
        if sim.vel(ball).y > 5.0 {
            bounced = true;
        }
        if bounced {
            peak_after_bounce = peak_after_bounce.max(sim.pos(ball).y);
        }
    }
    assert!(bounced && peak_after_bounce > 8.0, "an elastic ball comes back up (peak {peak_after_bounce})");

    // The same slope: a slippery block slides, a grippy one stays.
    let ramp_tilt = |friction: f32| {
        let mut sim = Sim::new();
        let ramp = sim.anchored(v(0.0, 0.0, 0.0), v(40.0, 1.0, 40.0));
        sim.set(ramp, "CFrame", Value::CF(Vec3::ZERO, Quat::from_rotation_z(0.3)));
        let block = sim.part(v(0.0, 1.5, 0.0), v(2.0, 1.0, 2.0));
        sim.set(block, "CFrame", Value::CF(v(0.0, 1.0, 0.0), Quat::from_rotation_z(0.3)));
        sim.set(ramp, "Friction", Value::Num(friction));
        sim.set(block, "Friction", Value::Num(friction));
        sim.run(120);
        sim.pos(block).x.abs()
    };
    let (slippery, grippy) = (ramp_tilt(0.0), ramp_tilt(1.5));
    assert!(slippery > 5.0, "no friction: it slides ({slippery})");
    assert!(grippy < 0.5, "plenty of friction: it stays ({grippy})");
}

#[test]
fn anchoring_and_unanchoring_a_part_while_it_runs() {
    let mut sim = Sim::new();
    let part = sim.part(v(0.0, 100.0, 0.0), Vec3::ONE);
    sim.run(20);
    sim.flag(part, "Anchored", true);
    sim.run(1);
    let frozen = sim.pos(part);
    sim.run(30);
    assert_eq!(sim.pos(part), frozen, "anchored: it hangs where it was");
    assert_eq!(sim.vel(part), Vec3::ZERO, "an anchored part has no velocity");
    sim.flag(part, "Anchored", false);
    sim.run(30);
    assert!(sim.pos(part).y < frozen.y - 5.0, "released: it falls again");
}

#[test]
fn resizing_a_part_while_it_runs_changes_where_it_rests() {
    let mut sim = Sim::new();
    sim.ground();
    let part = sim.part(v(0.0, 3.0, 0.0), v(2.0, 1.0, 2.0));
    sim.run_until(600, |s| s.sleeping(part));
    assert!(near(sim.pos(part).y, 0.5, 0.03));
    sim.set(part, "Size", Value::V3(v(2.0, 6.0, 2.0)));
    sim.run(2); // (the new shape wakes it)
    assert!(!sim.sleeping(part), "an edit to a resting part wakes it");
    sim.run_until(900, |s| s.sleeping(part));
    // (A part twice as tall as it is wide may stand or lie down: either way it is not where the thin one was.)
    assert!(sim.pos(part).y > 0.9, "a taller part rests higher: {}", sim.pos(part).y);
    // Changing the shape works the same way.
    sim.set(part, "Shape", Value::Enum("Ball".into()));
    sim.set(part, "Size", Value::V3(Vec3::splat(4.0)));
    sim.run(2);
    sim.run_until(900, |s| s.sleeping(part));
    assert!(near(sim.pos(part).y, 2.0, 0.05), "a ball of diameter 4 rests with its centre at its radius: {}", sim.pos(part).y);
}

#[test]
fn every_shape_rests_on_the_floor_at_the_height_its_geometry_gives() {
    let mut sim = Sim::new();
    sim.ground();
    let cases: [(&str, Vec3); 4] = [("Block", v(4.0, 2.0, 4.0)), ("Ball", Vec3::splat(4.0)), ("Ball", v(6.0, 2.0, 4.0)), ("Wedge", v(4.0, 2.0, 4.0))];
    let mut parts = Vec::new();
    for (i, (shape, size)) in cases.iter().enumerate() {
        let id = sim.part(v(i as f32 * 20.0, 5.0, 0.0), *size);
        sim.set(id, "Shape", Value::Enum((*shape).into()));
        parts.push(id);
    }
    sim.run_until(900, |s| parts.iter().all(|p| s.sleeping(*p)));
    assert!(parts.iter().all(|p| sim.sleeping(*p)), "everything came to rest");
    assert!(near(sim.pos(parts[0]).y, 1.0, 0.04), "block {}", sim.pos(parts[0]).y);
    assert!(near(sim.pos(parts[1]).y, 2.0, 0.04), "sphere {}", sim.pos(parts[1]).y);
    // A stretched ball rests on its shortest side: about half its height (its hull is a cage inside the ellipsoid, so a little lower).
    assert!((0.7..1.05).contains(&sim.pos(parts[2]).y), "ellipsoid {}", sim.pos(parts[2]).y);
    // A wedge rests flat on its floor face (its centre is half its height up) or on its slope - never inside the floor.
    assert!(sim.pos(parts[3]).y > 0.9, "wedge {}", sim.pos(parts[3]).y);
}

// ---- impulses ---------------------------------------------------------------------------------------

#[test]
fn an_impulse_changes_the_velocity_by_impulse_over_mass() {
    let mut sim = Sim::new();
    let part = sim.part(v(0.0, 100.0, 0.0), v(2.0, 1.0, 2.0)); // mass 4
    sim.set(sim.dm.workspace, "Gravity", Value::Num(0.0));
    sim.run(2);
    sim.in_frame(|b, dm| b.apply_impulse(dm, part, v(40.0, 0.0, 20.0), None)).unwrap();
    assert!(near(sim.vel(part).x, 10.0, 1e-3) && near(sim.vel(part).z, 5.0, 1e-3), "the velocity changed at once: {:?}", sim.vel(part));
    sim.run(60);
    assert!(near(sim.pos(part).x, 10.0, 0.5), "and it moves at that speed: {}", sim.pos(part).x);
    assert!(!sim.dm.flag(part, "Sleeping"));
}

#[test]
fn an_impulse_off_centre_spins_the_part_and_an_angular_impulse_spins_it_about_an_axis() {
    let mut sim = Sim::new();
    let part = sim.part(v(0.0, 100.0, 0.0), v(2.0, 2.0, 2.0));
    sim.set(sim.dm.workspace, "Gravity", Value::Num(0.0));
    sim.run(2);
    // A push along +X at a point above the centre turns it about Z (clockwise seen from +Z: negative).
    sim.in_frame(|b, dm| b.apply_impulse(dm, part, v(10.0, 0.0, 0.0), Some(v(0.0, 101.0, 0.0)))).unwrap();
    let spin = sim.dm.vec3(part, "AssemblyAngularVelocity");
    assert!(spin.z < -0.1 && spin.x.abs() < 1e-3 && spin.y.abs() < 1e-3, "{spin:?}");
    let before = sim.vel(part);
    assert!(before.x > 0.1, "{before:?}");

    let other = sim.part(v(50.0, 100.0, 0.0), v(2.0, 2.0, 2.0));
    sim.run(2);
    sim.in_frame(|b, dm| b.apply_angular_impulse(dm, other, v(0.0, 8.0, 0.0))).unwrap();
    assert!(sim.dm.vec3(other, "AssemblyAngularVelocity").y > 0.1);
    assert_eq!(sim.vel(other), Vec3::ZERO, "a pure spin does not move it");
}

#[test]
fn the_velocity_at_a_point_includes_the_turning() {
    let mut sim = Sim::new();
    let part = sim.part(v(0.0, 100.0, 0.0), Vec3::ONE);
    sim.set(sim.dm.workspace, "Gravity", Value::Num(0.0));
    sim.set(part, "AssemblyLinearVelocity", Value::V3(v(1.0, 0.0, 0.0)));
    sim.set(part, "AssemblyAngularVelocity", Value::V3(v(0.0, 2.0, 0.0)));
    sim.run(1);
    let at_centre = sim.in_frame(|b, dm| b.velocity_at(dm, part, sim_pos(dm, part))).unwrap();
    assert!((at_centre - v(1.0, 0.0, 0.0)).length() < 0.05, "{at_centre:?}");
    // 3 studs along +X from the centre, a turn of 2 rad/s about Y moves that point by 2 * 3 along -Z.
    let at = sim.in_frame(|b, dm| b.velocity_at(dm, part, sim_pos(dm, part) + v(3.0, 0.0, 0.0))).unwrap();
    assert!((at - v(1.0, 0.0, -6.0)).length() < 0.1, "{at:?}");
}

fn sim_pos(dm: &DataModel, id: Id) -> Vec3 {
    dm.vec3(id, "Position")
}

#[test]
fn impulses_are_refused_for_what_cannot_take_them() {
    let mut sim = Sim::new();
    let anchored = sim.anchored(v(0.0, 5.0, 0.0), Vec3::ONE);
    let part = sim.part(v(5.0, 5.0, 0.0), Vec3::ONE);
    let loose = sim.dm.create("Part").unwrap();
    sim.run(1);
    let err = |r: Result<(), String>| r.unwrap_err();
    assert!(err(sim.in_frame(|b, dm| b.apply_impulse(dm, anchored, v(1.0, 0.0, 0.0), None))).contains("anchored"));
    assert!(err(sim.in_frame(|b, dm| b.apply_impulse(dm, loose, v(1.0, 0.0, 0.0), None))).contains("not in the workspace"));
    assert!(err(sim.in_frame(|b, dm| b.apply_impulse(dm, part, v(f32::NAN, 0.0, 0.0), None))).contains("finite"));
    assert!(err(sim.in_frame(|b, dm| b.apply_impulse(dm, part, v(1.0, 0.0, 0.0), Some(v(0.0, f32::INFINITY, 0.0))))).contains("finite"));
    assert!(err(sim.in_frame(|b, dm| b.apply_angular_impulse(dm, anchored, v(1.0, 0.0, 0.0)))).contains("anchored"));
    // Outside a frame of scripts the world is not reachable: a part that has no body yet cannot be given one, and
    // the error says so (a part that has one can be pushed from anywhere).
    let fresh = sim.part(v(9.0, 5.0, 0.0), Vec3::ONE);
    assert!(err(sim.bridge.apply_impulse(&mut sim.dm, fresh, v(1.0, 0.0, 0.0), None)).contains("not running"));
}

#[test]
fn a_part_made_this_frame_can_be_pushed_at_once() {
    let mut sim = Sim::new();
    sim.set(sim.dm.workspace, "Gravity", Value::Num(0.0));
    sim.run(1);
    let ws = sim.dm.workspace;
    let fresh = sim.in_frame(|b, dm| {
        let id = dm.create("Part").unwrap();
        dm.set_prop(id, "Position", Value::V3(v(0.0, 50.0, 0.0))).unwrap();
        dm.set_parent(id, Some(ws)).unwrap();
        // (A script pushes the part on the line after it made it: no step has seen it yet.)
        b.apply_impulse(dm, id, v(0.0, 10.0, 0.0), None).unwrap();
        id
    });
    assert!(sim.vel(fresh).y > 0.5, "{:?}", sim.vel(fresh));
}

// ---- queries ----------------------------------------------------------------------------------------

fn params() -> QueryParams {
    QueryParams::default()
}

#[test]
fn a_ray_hits_the_nearest_part_and_reports_where_and_how_far() {
    let mut sim = Sim::new();
    sim.ground();
    let near_box = sim.anchored(v(0.0, 5.0, 0.0), v(2.0, 2.0, 2.0));
    let far_box = sim.anchored(v(0.0, 5.0, 10.0), v(2.0, 2.0, 2.0));
    sim.run(1);
    let hit = sim.in_frame(|b, dm| b.raycast(dm, v(0.0, 5.0, -10.0), v(0.0, 0.0, 30.0), &params())).unwrap().expect("a hit");
    assert_eq!(hit.id, near_box);
    assert!((hit.position - v(0.0, 5.0, -1.0)).length() < 1e-3, "{:?}", hit.position);
    assert!((hit.normal - v(0.0, 0.0, -1.0)).length() < 1e-3, "{:?}", hit.normal);
    assert!(near(hit.distance, 9.0, 1e-3), "{}", hit.distance);
    // The direction is also the length: a short ray stops short.
    assert!(sim.in_frame(|b, dm| b.raycast(dm, v(0.0, 5.0, -10.0), v(0.0, 0.0, 5.0), &params())).unwrap().is_none());
    // Straight down onto the floor.
    let down = sim.in_frame(|b, dm| b.raycast(dm, v(20.0, 10.0, 0.0), v(0.0, -50.0, 0.0), &params())).unwrap().unwrap();
    assert!(near(down.position.y, 0.0, 1e-3) && near(down.distance, 10.0, 1e-3));
    let _ = far_box;
}

#[test]
fn a_ray_is_found_only_when_the_part_allows_it_and_the_filter_lets_it() {
    let mut sim = Sim::new();
    let a = sim.anchored(v(0.0, 5.0, 0.0), v(2.0, 2.0, 2.0));
    let b = sim.anchored(v(0.0, 5.0, 10.0), v(2.0, 2.0, 2.0));
    let model = sim.dm.create("Model").unwrap();
    sim.dm.set_parent(model, Some(sim.dm.workspace)).unwrap();
    sim.dm.set_parent(b, Some(model)).unwrap();
    sim.run(1);
    let cast = |sim: &mut Sim, p: QueryParams| sim.in_frame(|br, dm| br.raycast(dm, v(0.0, 5.0, -10.0), v(0.0, 0.0, 40.0), &p)).unwrap().map(|h| h.id);
    assert_eq!(cast(&mut sim, params()), Some(a));
    // Exclude: the listed instance, and everything inside a listed model, is ignored.
    assert_eq!(cast(&mut sim, QueryParams { filter: vec![a], ..params() }), Some(b));
    assert_eq!(cast(&mut sim, QueryParams { filter: vec![a, model], ..params() }), None);
    // Include: only the listed ones (and what is inside them) count.
    assert_eq!(cast(&mut sim, QueryParams { filter: vec![model], include: true, ..params() }), Some(b));
    assert_eq!(cast(&mut sim, QueryParams { include: true, ..params() }), None, "include nothing: nothing");
    // CanQuery off hides the part from every query, whatever it collides with.
    sim.flag(a, "CanQuery", false);
    sim.run(1);
    assert_eq!(cast(&mut sim, params()), Some(b));
    // A part that does not collide is still found, unless the query says to respect CanCollide.
    sim.flag(b, "CanCollide", false);
    sim.run(1);
    assert_eq!(cast(&mut sim, params()), Some(b));
    assert_eq!(cast(&mut sim, QueryParams { respect_can_collide: true, ..params() }), None);
    // Even one that cannot be touched either.
    sim.flag(b, "CanTouch", false);
    sim.run(1);
    assert_eq!(cast(&mut sim, params()), Some(b));
}

#[test]
fn rays_follow_collision_groups() {
    let mut sim = Sim::new();
    let ws_service = sim.dm.service("PhysicsService").unwrap();
    let mut groups = groups::CollisionGroups::default();
    groups.register("Ghosts").unwrap();
    groups.set_collidable("Default", "Ghosts", false).unwrap();
    sim.set(ws_service, "CollisionGroups", Value::Str(groups.encode()));
    let ghost = sim.anchored(v(0.0, 5.0, 0.0), v(2.0, 2.0, 2.0));
    sim.set(ghost, "CollisionGroup", Value::Str("Ghosts".into()));
    let solid = sim.anchored(v(0.0, 5.0, 10.0), v(2.0, 2.0, 2.0));
    sim.run(1);
    let cast = |sim: &mut Sim, group: Option<&str>| {
        let p = QueryParams { group: group.map(String::from), ..params() };
        sim.in_frame(|br, dm| br.raycast(dm, v(0.0, 5.0, -10.0), v(0.0, 0.0, 40.0), &p)).map(|h| h.map(|h| h.id))
    };
    assert_eq!(cast(&mut sim, None), Ok(Some(ghost)), "without a group, every part is seen");
    assert_eq!(cast(&mut sim, Some("Default")), Ok(Some(solid)), "a ray of the Default group does not see what Default does not collide with");
    assert_eq!(cast(&mut sim, Some("Ghosts")), Ok(Some(ghost)), "ghosts collide with ghosts");
    assert!(cast(&mut sim, Some("Nope")).unwrap_err().contains("not a registered"));
}

#[test]
fn bad_rays_are_refused_or_empty_and_never_panic() {
    let mut sim = Sim::new();
    sim.ground();
    sim.run(1);
    sim.in_frame(|b, dm| {
        assert!(b.raycast(dm, v(f32::NAN, 0.0, 0.0), v(0.0, -1.0, 0.0), &params()).unwrap_err().contains("finite"));
        assert!(b.raycast(dm, Vec3::ZERO, v(f32::INFINITY, 0.0, 0.0), &params()).unwrap_err().contains("finite"));
        assert_eq!(b.raycast(dm, Vec3::new(0.0, 5.0, 0.0), Vec3::ZERO, &params()), Ok(None), "a zero-length ray hits nothing");
        assert_eq!(b.raycast(dm, v(0.0, 5.0, 0.0), v(0.0, -1.0e-9, 0.0), &params()), Ok(None));
        assert!(b.shapecast(dm, Vec3::ZERO, CastShape::Sphere { radius: -1.0 }, v(0.0, -5.0, 0.0), &params()).is_err());
        assert!(b.shapecast(dm, Vec3::ZERO, CastShape::Block { size: v(0.0, 1.0, 1.0), rotation: Quat::IDENTITY }, v(0.0, -5.0, 0.0), &params()).is_err());
        assert!(b.overlap(dm, v(f32::NAN, 0.0, 0.0), CastShape::Sphere { radius: 1.0 }, &params()).is_err());
        // A huge but finite ray is fine.
        assert!(b.raycast(dm, v(0.0, 5.0, 0.0), v(0.0, -1.0e7, 0.0), &params()).is_ok());
    });
}

#[test]
fn shape_casts_sweep_a_solid_and_report_the_first_part_it_hits() {
    let mut sim = Sim::new();
    sim.ground();
    let wall = sim.anchored(v(0.0, 3.0, 10.0), v(10.0, 6.0, 1.0));
    sim.run(1);
    // A ray 1.4 studs above the floor passes over a thin gap a ball of radius 1 cannot: the ball hits the floor.
    let ray = sim.in_frame(|b, dm| b.raycast(dm, v(0.0, 0.5, -5.0), v(0.0, 0.0, 30.0), &params())).unwrap().unwrap();
    assert_eq!(ray.id, wall);
    let ball = sim.in_frame(|b, dm| b.shapecast(dm, v(0.0, 0.5, -5.0), CastShape::Sphere { radius: 1.0 }, v(0.0, -5.0, 0.0), &params())).unwrap().unwrap();
    assert_ne!(ball.id, wall, "swept down, the ball finds the floor");
    assert!(near(ball.distance, 0.0, 0.6) || ball.distance < 1.0, "{}", ball.distance);
    let to_wall = sim.in_frame(|b, dm| b.shapecast(dm, v(0.0, 3.0, 0.0), CastShape::Sphere { radius: 1.0 }, v(0.0, 0.0, 30.0), &params())).unwrap().unwrap();
    assert_eq!(to_wall.id, wall);
    assert!(near(to_wall.distance, 8.5, 0.05), "the ball's front reaches the wall's near face (z = 9.5) after 8.5: {}", to_wall.distance);
    let block = sim.in_frame(|b, dm| b.shapecast(dm, v(0.0, 3.0, 0.0), CastShape::Block { size: v(2.0, 2.0, 4.0), rotation: Quat::IDENTITY }, v(0.0, 0.0, 30.0), &params())).unwrap().unwrap();
    assert!(near(block.distance, 7.5, 0.05), "a block 4 long reaches it sooner: {}", block.distance);
    let missed = sim.in_frame(|b, dm| b.shapecast(dm, v(40.0, 3.0, 0.0), CastShape::Sphere { radius: 1.0 }, v(0.0, 0.0, 30.0), &params())).unwrap();
    assert!(missed.is_none());
}

#[test]
fn overlap_queries_find_the_parts_in_a_box_a_sphere_or_another_part() {
    let mut sim = Sim::new();
    let a = sim.anchored(v(0.0, 5.0, 0.0), v(2.0, 2.0, 2.0));
    let b = sim.anchored(v(3.0, 5.0, 0.0), v(2.0, 2.0, 2.0));
    let c = sim.anchored(v(30.0, 5.0, 0.0), v(2.0, 2.0, 2.0));
    sim.run(1);
    let bx = sim.in_frame(|br, dm| br.overlap(dm, v(1.5, 5.0, 0.0), CastShape::Block { size: v(8.0, 4.0, 4.0), rotation: Quat::IDENTITY }, &params())).unwrap();
    assert_eq!(bx, {
        let mut e = vec![a, b];
        e.sort_unstable();
        e
    });
    let sph = sim.in_frame(|br, dm| br.overlap(dm, v(30.0, 5.0, 0.0), CastShape::Sphere { radius: 0.5 }, &params())).unwrap();
    assert_eq!(sph, vec![c]);
    // A part overlapping a part: its neighbour, not itself.
    sim.set(b, "Position", Value::V3(v(1.5, 5.0, 0.0)));
    sim.run(1);
    assert_eq!(sim.in_frame(|br, dm| br.overlap_part(dm, a, &params())).unwrap(), vec![b]);
    // The limit and the filter apply.
    let capped = sim.in_frame(|br, dm| br.overlap(dm, v(1.0, 5.0, 0.0), CastShape::Sphere { radius: 50.0 }, &QueryParams { max_parts: 2, ..params() })).unwrap();
    assert_eq!(capped.len(), 2);
    let without_a = sim.in_frame(|br, dm| br.overlap(dm, v(1.0, 5.0, 0.0), CastShape::Sphere { radius: 3.0 }, &QueryParams { filter: vec![a], ..params() })).unwrap();
    assert_eq!(without_a, vec![b]);
    // A turned box finds what its corners reach.
    let turned = sim.in_frame(|br, dm| br.overlap(dm, v(30.0, 5.0, 5.0), CastShape::Block { size: v(1.0, 1.0, 12.0), rotation: Quat::IDENTITY }, &params()));
    assert_eq!(turned.unwrap(), vec![c], "a long thin box reaches across");
}

#[test]
fn touching_parts_lists_what_a_part_rests_on_and_what_overlaps_it() {
    let mut sim = Sim::new();
    let ground = sim.ground();
    let part = sim.part(v(0.0, 3.0, 0.0), Vec3::ONE);
    let ghost = sim.part(v(0.0, 0.5, 0.0), Vec3::ONE);
    sim.flag(ghost, "CanCollide", false);
    sim.flag(ghost, "Anchored", true);
    sim.run_until(600, |s| s.sleeping(part));
    let mut touching = sim.in_frame(|b, dm| b.touching_parts(dm, part)).unwrap();
    touching.sort_unstable();
    let mut want = vec![ground, ghost];
    want.sort_unstable();
    assert_eq!(touching, want);
    sim.flag(ground, "CanTouch", false);
    sim.run(1);
    assert!(!sim.in_frame(|b, dm| b.touching_parts(dm, part)).unwrap().contains(&ground), "a part that cannot be touched is not listed");
}
// ---- constraints ------------------------------------------------------------------------------------

impl Sim {
    fn constraint(&mut self, class: &str, a: Option<Id>, b: Option<Id>, props: &[(&str, Value)]) -> Id {
        let id = self.dm.create(class).unwrap();
        self.set(id, "Part0", Value::Ref(a));
        self.set(id, "Part1", Value::Ref(b));
        for (k, val) in props {
            self.set(id, k, val.clone());
        }
        self.dm.set_parent(id, Some(self.dm.workspace)).unwrap();
        id
    }

    fn no_gravity(&mut self) {
        self.set(self.dm.workspace, "Gravity", Value::Num(0.0));
    }
}

fn dist(a: Vec3, b: Vec3) -> f32 {
    (a - b).length()
}

#[test]
fn a_weld_holds_a_part_to_another_and_lets_go_when_disabled_or_destroyed() {
    let mut sim = Sim::new();
    let a = sim.anchored(v(0.0, 50.0, 0.0), Vec3::ONE);
    let b = sim.part(v(0.0, 48.0, 0.0), Vec3::ONE);
    let weld = sim.constraint("WeldConstraint", Some(a), Some(b), &[]);
    sim.run(120);
    assert!(dist(sim.pos(b), v(0.0, 48.0, 0.0)) < 0.05, "welded to something anchored: it does not fall, got {:?}", sim.pos(b));
    assert!(sim.bridge.joint_active(weld));
    sim.flag(weld, "Enabled", false);
    sim.run(30);
    assert!(!sim.bridge.joint_active(weld));
    assert!(sim.pos(b).y < 46.0, "disabled: it falls");

    // Enabled again: it welds the parts as they are now.
    sim.set(weld, "Enabled", Value::Bool(true));
    sim.run(2);
    assert!(sim.bridge.joint_active(weld));
    let held = sim.pos(b);
    sim.run(60);
    assert!(dist(sim.pos(b), held) < 0.3, "{:?} -> {:?}", held, sim.pos(b));

    sim.dm.destroy(weld);
    sim.run(30);
    assert!(sim.pos(b).y < held.y - 5.0, "the constraint is gone and so is the hold");
    assert_eq!(sim.stats().joints, 0);
}

#[test]
fn two_welded_parts_keep_their_distance_and_orientation_while_they_fall() {
    let mut sim = Sim::new();
    let a = sim.part(v(0.0, 60.0, 0.0), Vec3::ONE);
    let b = sim.part(v(3.0, 60.0, 0.0), Vec3::ONE);
    sim.constraint("WeldConstraint", Some(a), Some(b), &[]);
    sim.set(a, "AssemblyAngularVelocity", Value::V3(v(0.0, 0.0, 0.0)));
    sim.run(60);
    assert!(sim.pos(a).y < 50.0, "they fall");
    assert!(near(dist(sim.pos(a), sim.pos(b)), 3.0, 0.05), "{:?} {:?}", sim.pos(a), sim.pos(b));
    // Pushed off centre, the pair turns as one body: the distance still holds.
    sim.in_frame(|br, dm| br.apply_impulse(dm, a, v(0.0, 0.0, 30.0), Some(v(-0.5, 20.0, 0.0)))).unwrap();
    sim.run(60);
    assert!(near(dist(sim.pos(a), sim.pos(b)), 3.0, 0.1), "{:?} {:?}", sim.pos(a), sim.pos(b));
}

#[test]
fn a_hinge_swings_about_its_axis_and_its_limits_hold() {
    let mut sim = Sim::new();
    let a = sim.anchored(v(0.0, 20.0, 0.0), Vec3::ONE);
    let b = sim.part(v(2.0, 20.0, 0.0), Vec3::ONE);
    // The hinge is at (1, 20, 0): 1 stud from each part, its axis the world's Z (the frame's X turned a quarter about Y).
    let hinge = sim.constraint(
        "HingeConstraint",
        Some(a),
        Some(b),
        &[("Anchor0", Value::V3(v(1.0, 0.0, 0.0))), ("Anchor1", Value::V3(v(-1.0, 0.0, 0.0))), ("Rotation0", Value::V3(v(0.0, 90.0, 0.0))), ("Rotation1", Value::V3(v(0.0, 90.0, 0.0)))],
    );
    sim.run(300);
    let p = sim.pos(b);
    assert!(near(dist(p, v(1.0, 20.0, 0.0)), 1.0, 0.1), "it stays one stud from the hinge: {p:?}");
    assert!(p.z.abs() < 0.1, "and swings in the plane the axis leaves free: {p:?}");
    assert!(p.y < 20.5, "it swung down (under gravity)");

    // Limits: within ten degrees of horizontal.
    sim.set(hinge, "LimitsEnabled", Value::Bool(true));
    sim.set(hinge, "LowerAngle", Value::Num(-10.0));
    sim.set(hinge, "UpperAngle", Value::Num(10.0));
    sim.set(b, "CFrame", Value::CF(v(2.0, 20.0, 0.0), Quat::IDENTITY));
    sim.set(b, "AssemblyLinearVelocity", Value::V3(Vec3::ZERO));
    sim.run(300);
    let p = sim.pos(b);
    let angle = (p.y - 20.0).atan2(p.x - 1.0).to_degrees();
    assert!(angle.abs() <= 12.0, "it hangs at the limit instead of falling to -90: {angle}");
    assert!(angle < -5.0, "and it did reach it: {angle}");
}

#[test]
fn a_motor_turns_a_hinge_at_the_speed_asked() {
    let mut sim = Sim::new();
    sim.no_gravity();
    let a = sim.anchored(v(0.0, 20.0, 0.0), Vec3::ONE);
    let b = sim.part(v(0.0, 20.0, 3.0), v(1.0, 1.0, 1.0));
    sim.constraint("HingeConstraint", Some(a), Some(b), &[("Anchor1", Value::V3(v(0.0, 0.0, -3.0))), ("MotorEnabled", Value::Bool(true)), ("MotorSpeed", Value::Num(2.0))]);
    sim.run(60);
    let spin = sim.dm.vec3(b, "AssemblyAngularVelocity");
    assert!(near(spin.length(), 2.0, 0.2), "{spin:?}");
    assert!(spin.x.abs() > 1.8, "about the frame's X axis: {spin:?}");
}

#[test]
fn a_rod_keeps_a_fixed_distance_a_rope_allows_slack_and_a_spring_settles_where_the_forces_balance() {
    let mut sim = Sim::new();
    let top = sim.anchored(v(0.0, 100.0, 0.0), Vec3::ONE);
    let rod_end = sim.part(v(-20.0, 98.0, 0.0), Vec3::ONE);
    let rope_end = sim.part(v(0.0, 99.0, 0.0), Vec3::ONE);
    let spring_end = sim.part(v(20.0, 95.0, 0.0), Vec3::ONE);
    // (Parts anchored a little apart so the constraints join frames that are not at one place.)
    let rod_top = sim.anchored(v(-20.0, 100.0, 0.0), Vec3::ONE);
    let spring_top = sim.anchored(v(20.0, 100.0, 0.0), Vec3::ONE);
    sim.constraint("RodConstraint", Some(rod_top), Some(rod_end), &[("Length", Value::Num(5.0))]);
    sim.constraint("RopeConstraint", Some(top), Some(rope_end), &[("Length", Value::Num(8.0))]);
    sim.constraint("SpringConstraint", Some(spring_top), Some(spring_end), &[("FreeLength", Value::Num(5.0)), ("Frequency", Value::Num(3.0)), ("DampingRatio", Value::Num(1.0))]);
    sim.set(rod_end, "AssemblyLinearVelocity", Value::V3(v(10.0, 0.0, 0.0)));
    let mut rod_error = 0.0f32;
    let mut rope_max = 0.0f32;
    for i in 0..600 {
        sim.step();
        if i > 30 {
            rod_error = rod_error.max((dist(sim.pos(rod_end), sim.pos(rod_top)) - 5.0).abs());
        }
        rope_max = rope_max.max(dist(sim.pos(rope_end), sim.pos(top)));
    }
    assert!(rod_error < 0.15, "the rod stays 5 studs long even when it swings: error {rod_error}");
    assert!(rope_max < 8.3, "the rope never stretches past 8: {rope_max}");
    assert!(dist(sim.pos(rope_end), sim.pos(top)) > 7.4, "and hangs taut at the end of it: {}", dist(sim.pos(rope_end), sim.pos(top)));
    // A spring with frequency f stretches by g / (2 pi f)^2 under its own weight.
    let stretch = dist(sim.pos(spring_end), sim.pos(spring_top)) - 5.0;
    let expected = 196.2 / (2.0 * std::f32::consts::PI * 3.0).powi(2);
    assert!(near(stretch, expected, 0.2), "stretch {stretch}, expected about {expected}");
}

#[test]
fn a_prismatic_constraint_lets_a_part_slide_along_one_axis_between_its_limits() {
    let mut sim = Sim::new();
    let rail = sim.anchored(v(0.0, 50.0, 0.0), Vec3::ONE);
    let slider = sim.part(v(0.0, 50.0, 0.0), Vec3::ONE);
    // The frame's X axis turned to point up: the slider moves along Y, from 4 studs below to the rail.
    sim.constraint(
        "PrismaticConstraint",
        Some(rail),
        Some(slider),
        &[
            ("Rotation0", Value::V3(v(0.0, 0.0, 90.0))),
            ("Rotation1", Value::V3(v(0.0, 0.0, 90.0))),
            ("LimitsEnabled", Value::Bool(true)),
            ("LowerLimit", Value::Num(-4.0)),
            ("UpperLimit", Value::Num(0.0)),
        ],
    );
    sim.run(300);
    let p = sim.pos(slider);
    assert!(near(p.y, 46.0, 0.15), "it slid down to the lower limit: {p:?}");
    assert!(p.x.abs() < 0.05 && p.z.abs() < 0.05, "and nowhere else: {p:?}");
}

#[test]
fn a_ball_socket_lets_a_part_swing_freely_about_a_point() {
    let mut sim = Sim::new();
    let a = sim.anchored(v(0.0, 50.0, 0.0), Vec3::ONE);
    let b = sim.part(v(2.0, 50.0, 0.0), Vec3::ONE);
    sim.constraint("BallSocketConstraint", Some(a), Some(b), &[("Anchor0", Value::V3(v(1.0, 0.0, 0.0))), ("Anchor1", Value::V3(v(-1.0, 0.0, 0.0)))]);
    sim.set(b, "AssemblyLinearVelocity", Value::V3(v(0.0, 0.0, 15.0)));
    sim.run(180);
    assert!(near(dist(sim.pos(b), v(1.0, 50.0, 0.0)), 1.0, 0.1), "it stays one stud from the socket: {:?}", sim.pos(b));
    assert!(sim.pos(b).y < 50.0, "and swings down");
}

#[test]
fn a_constraint_without_a_second_part_joins_the_first_to_the_world() {
    let mut sim = Sim::new();
    let ball = sim.part(v(0.0, 28.0, 0.0), Vec3::ONE);
    // The rope's other end is the world point (0, 30, 0).
    sim.constraint("RopeConstraint", Some(ball), None, &[("Length", Value::Num(6.0)), ("Anchor1", Value::V3(v(0.0, 30.0, 0.0)))]);
    sim.run(300);
    let d = dist(sim.pos(ball), v(0.0, 30.0, 0.0));
    assert!(d > 5.3 && d < 6.2, "it hangs from the point by a 6-stud rope: {d}");
}

#[test]
fn a_constraint_is_only_active_in_the_workspace_with_both_parts_and_comes_back_with_them() {
    let mut sim = Sim::new();
    let a = sim.anchored(v(0.0, 50.0, 0.0), Vec3::ONE);
    let b = sim.part(v(0.0, 48.0, 0.0), Vec3::ONE);
    let weld = sim.constraint("WeldConstraint", Some(a), Some(b), &[]);
    sim.run(2);
    assert!(sim.bridge.joint_active(weld));
    // Out of the workspace the constraint does nothing.
    let storage = sim.dm.service("ServerStorage").unwrap();
    sim.dm.set_parent(weld, Some(storage)).unwrap();
    sim.run(2);
    assert!(!sim.bridge.joint_active(weld));
    sim.dm.set_parent(weld, Some(sim.dm.workspace)).unwrap();
    sim.run(2);
    assert!(sim.bridge.joint_active(weld));
    // A part that goes takes the joint with it; one that comes back (re-parented) gets it back.
    sim.dm.set_parent(b, Some(storage)).unwrap();
    sim.run(2);
    assert!(!sim.bridge.joint_active(weld));
    sim.set(b, "CFrame", Value::CF(v(0.0, 48.0, 0.0), Quat::IDENTITY));
    sim.dm.set_parent(b, Some(sim.dm.workspace)).unwrap();
    sim.run(60);
    assert!(sim.bridge.joint_active(weld));
    assert!(dist(sim.pos(b), v(0.0, 48.0, 0.0)) < 0.1, "welded again: {:?}", sim.pos(b));
    // Destroying a part releases the constraint without trouble.
    sim.dm.destroy(a);
    sim.run(5);
    assert!(!sim.bridge.joint_active(weld));
    assert!(sim.pos(b).y < 48.0);
}

#[test]
fn constraints_with_nonsense_do_nothing_and_do_not_break_the_world() {
    let mut sim = Sim::new();
    let a = sim.anchored(v(0.0, 50.0, 0.0), Vec3::ONE);
    let same = sim.constraint("WeldConstraint", Some(a), Some(a), &[]);
    let nobody = sim.constraint("HingeConstraint", None, None, &[]);
    let model = sim.dm.create("Model").unwrap();
    assert!(sim.dm.set_prop(same, "Part1", Value::Ref(Some(model))).is_err(), "a model is not a part");
    sim.run(3);
    assert!(!sim.bridge.joint_active(same) && !sim.bridge.joint_active(nobody));
    // Out-of-range limits are put in range.
    let b = sim.part(v(2.0, 50.0, 0.0), Vec3::ONE);
    sim.constraint("HingeConstraint", Some(a), Some(b), &[("LimitsEnabled", Value::Bool(true)), ("LowerAngle", Value::Num(900.0)), ("UpperAngle", Value::Num(-900.0))]);
    sim.run(30);
    assert!(sim.pos(b).y.is_finite());
}

// ---- touches ----------------------------------------------------------------------------------------

impl Sim {
    fn events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.dm.events)
    }

    fn touched(events: &[Event], part: Id, other: Id) -> usize {
        events.iter().filter(|e| matches!(e, Event::Touched(p, o) if *p == part && *o == other)).count()
    }

    fn untouched(events: &[Event], part: Id, other: Id) -> usize {
        events.iter().filter(|e| matches!(e, Event::TouchEnded(p, o) if *p == part && *o == other)).count()
    }
}

#[test]
fn touched_and_touch_ended_fire_once_for_each_listening_part_when_parts_meet_and_part() {
    let mut sim = Sim::new();
    let ground = sim.ground();
    let ball = sim.part(v(0.0, 6.0, 0.0), Vec3::splat(2.0));
    sim.dm.watch_touch(ground);
    sim.dm.watch_touch(ball);
    sim.run(5);
    sim.events();
    let mut seen = Vec::new();
    for _ in 0..200 {
        sim.step();
        seen.extend(sim.events());
    }
    assert_eq!(Sim::touched(&seen, ground, ball), 1, "{seen:?}");
    assert_eq!(Sim::touched(&seen, ball, ground), 1, "both parts hear it, once");
    assert_eq!(Sim::untouched(&seen, ball, ground), 0, "it is still resting there");
    // Throw it up: it leaves, and both hear that too.
    sim.set(ball, "AssemblyLinearVelocity", Value::V3(v(0.0, 80.0, 0.0)));
    let mut seen = Vec::new();
    for _ in 0..20 {
        sim.step();
        seen.extend(sim.events());
    }
    assert_eq!(Sim::untouched(&seen, ground, ball), 1, "{seen:?}");
    assert_eq!(Sim::untouched(&seen, ball, ground), 1);
    assert!(sim.stats().touch_events <= 2);
}

#[test]
fn only_parts_somebody_listens_to_get_events_and_both_parts_must_allow_touching() {
    let mut sim = Sim::new();
    let ground = sim.ground();
    let quiet = sim.part(v(-10.0, 3.0, 0.0), Vec3::ONE);
    let listened = sim.part(v(10.0, 3.0, 0.0), Vec3::ONE);
    let blocked = sim.part(v(30.0, 3.0, 0.0), Vec3::ONE);
    sim.dm.watch_touch(listened);
    sim.dm.watch_touch(blocked);
    sim.flag(blocked, "CanTouch", false);
    sim.run(200);
    let seen = sim.events();
    assert_eq!(Sim::touched(&seen, listened, ground), 1);
    assert!(seen.iter().all(|e| !matches!(e, Event::Touched(p, _) if *p == quiet)), "nobody listens to the quiet one: {seen:?}");
    assert!(seen.iter().all(|e| !matches!(e, Event::Touched(p, _) if *p == blocked)), "CanTouch off: no events");
    // Turning CanTouch off on the *other* part also silences the pair.
    sim.flag(listened, "CanTouch", true);
    sim.flag(ground, "CanTouch", false);
    sim.set(listened, "AssemblyLinearVelocity", Value::V3(v(0.0, 60.0, 0.0)));
    sim.run(120);
    let seen = sim.events();
    assert_eq!(Sim::touched(&seen, listened, ground), 0, "the ground cannot be touched now: {seen:?}");
}

#[test]
fn a_part_that_does_not_collide_is_a_sensor_that_reports_what_passes_through_it() {
    let mut sim = Sim::new();
    sim.ground();
    let zone = sim.anchored(v(0.0, 10.0, 0.0), v(10.0, 4.0, 10.0));
    sim.flag(zone, "CanCollide", false);
    sim.dm.watch_touch(zone);
    let ball = sim.part(v(0.0, 30.0, 0.0), Vec3::splat(2.0));
    let mut seen = Vec::new();
    let mut y_inside = None;
    for _ in 0..300 {
        sim.step();
        seen.extend(sim.events());
        if (sim.pos(ball).y - 10.0).abs() < 1.0 {
            y_inside = Some(sim.pos(ball).y);
        }
    }
    assert!(y_inside.is_some(), "the ball fell right through the zone (it does not collide)");
    assert_eq!(Sim::touched(&seen, zone, ball), 1, "{seen:?}");
    assert_eq!(Sim::untouched(&seen, zone, ball), 1, "it left again");
    assert_eq!(sim.stats().joints, 0);
}

#[test]
fn a_touching_part_that_is_destroyed_ends_the_touch_for_the_part_left_behind() {
    let mut sim = Sim::new();
    let ground = sim.ground();
    let ball = sim.part(v(0.0, 3.0, 0.0), Vec3::splat(2.0));
    sim.dm.watch_touch(ground);
    sim.run(200);
    sim.events();
    sim.dm.destroy(ball);
    let mut seen = Vec::new();
    for _ in 0..5 {
        sim.step();
        seen.extend(sim.events());
    }
    assert_eq!(seen.iter().filter(|e| matches!(e, Event::TouchEnded(p, _) if *p == ground)).count(), 1, "{seen:?}");
}

#[test]
fn two_anchored_solid_parts_do_not_report_touches() {
    // (A documented limit of Box3D: contact events need a part that can move.)
    let mut sim = Sim::new();
    let a = sim.anchored(v(0.0, 5.0, 0.0), Vec3::splat(2.0));
    let b = sim.anchored(v(1.0, 5.0, 0.0), Vec3::splat(2.0));
    sim.dm.watch_touch(a);
    sim.dm.watch_touch(b);
    sim.run(10);
    assert!(sim.events().is_empty());
}
// ---- lifecycle --------------------------------------------------------------------------------------

#[test]
fn spawning_and_destroying_parts_over_and_over_leaves_nothing_behind() {
    let mut sim = Sim::new();
    sim.ground();
    sim.run(1);
    let baseline = sim.physics.world().counters().body_count;
    for round in 0..6 {
        let parts: Vec<Id> = (0..150).map(|i| sim.part(v((i % 15) as f32 * 3.0, 5.0 + (i / 15) as f32 * 2.0, 0.0), Vec3::splat(1.5))).collect();
        sim.run(40);
        assert!(sim.bodies() >= 151, "round {round}: {}", sim.bodies());
        for p in parts {
            sim.dm.destroy(p);
        }
        sim.run(3);
        assert_eq!(sim.bodies(), 1, "round {round}: only the ground is left");
        assert_eq!(sim.bridge.by_shape.len(), 1, "no shape is remembered for a part that is gone");
        assert_eq!(sim.physics.world().counters().body_count, baseline, "and the world holds no body for them");
        assert_eq!(sim.dm.phys.pending(), 0);
    }
}

#[test]
fn destroying_a_part_in_the_middle_of_a_step_sequence_is_harmless_and_slots_are_reused_safely() {
    let mut sim = Sim::new();
    sim.ground();
    let a = sim.part(v(0.0, 5.0, 0.0), Vec3::ONE);
    sim.run(3);
    sim.dm.destroy(a);
    let b = sim.part(v(5.0, 5.0, 0.0), Vec3::ONE); // (takes the slot of the one that was destroyed)
    assert_eq!(a.slot, b.slot);
    sim.run(30);
    assert!(sim.pos(b).y < 5.0 && sim.bridge.parts.contains_key(&b) && !sim.bridge.parts.contains_key(&a));
    assert_eq!(sim.bodies(), 2);
}

#[test]
fn leaving_the_workspace_removes_the_body_and_coming_back_makes_a_new_one() {
    let mut sim = Sim::new();
    sim.ground();
    let part = sim.part(v(0.0, 40.0, 0.0), Vec3::ONE);
    let model = sim.dm.create("Model").unwrap();
    sim.dm.set_parent(model, Some(sim.dm.workspace)).unwrap();
    sim.run(10);
    let storage = sim.dm.service("ServerStorage").unwrap();
    sim.dm.set_parent(part, Some(storage)).unwrap();
    sim.run(2);
    assert_eq!(sim.bodies(), 1);
    let frozen = sim.pos(part);
    sim.run(30);
    assert_eq!(sim.pos(part), frozen, "out of the world it does not move");
    // Back in, inside a model: it is simulated again, from where it was.
    sim.dm.set_parent(part, Some(model)).unwrap();
    sim.run(30);
    assert_eq!(sim.bodies(), 2);
    assert!(sim.pos(part).y < frozen.y - 5.0);
    // Moving a model moves what it holds... the parts are re-found where they now are.
    let before = sim.pos(part);
    sim.dm.pivot_to(model, (v(100.0, 100.0, 0.0), Quat::IDENTITY));
    sim.run(2);
    assert!(sim.pos(part).x > before.x + 50.0, "teleported with its model: {:?}", sim.pos(part));
}

#[test]
fn a_part_that_falls_out_of_the_world_is_destroyed() {
    let mut sim = Sim::new();
    let part = sim.part(v(0.0, -480.0, 0.0), Vec3::ONE);
    sim.run(10);
    assert!(sim.dm.exists(part));
    sim.run(40);
    assert!(!sim.dm.exists(part), "it fell below FallenPartsDestroyHeight (-500)");
    sim.run(3);
    assert_eq!(sim.bodies(), 0);
    let ws = sim.dm.workspace;
    sim.set(ws, "FallenPartsDestroyHeight", Value::Num(-100000.0));
    let again = sim.part(v(0.0, -480.0, 0.0), Vec3::ONE);
    sim.run(60);
    assert!(sim.dm.exists(again), "the height is a property");
}

#[test]
fn loading_a_scene_drops_the_old_world_and_builds_the_new_one() {
    let mut sim = Sim::new();
    sim.ground();
    let part = sim.part(v(0.0, 5.0, 0.0), Vec3::ONE);
    sim.run(5);
    let place = sim.dm.to_place("x");
    sim.dm.destroy(part);
    sim.dm.load_place(&place).unwrap();
    sim.bridge.reset();
    sim.bridge.attach(&mut sim.dm);
    sim.run(3);
    assert_eq!(sim.bodies(), 2, "the ground and the part, as the scene has them");
    sim.run(120);
    let loaded = sim.dm.find_first_child(sim.dm.workspace, "Part", false).unwrap();
    assert!(sim.dm.exists(loaded));
}

#[test]
fn the_world_can_go_before_the_bridge_without_harm() {
    let mut sim = Sim::new();
    sim.ground();
    let a = sim.part(v(0.0, 5.0, 0.0), Vec3::ONE);
    let b = sim.part(v(0.0, 8.0, 0.0), Vec3::ONE);
    sim.constraint("RodConstraint", Some(a), Some(b), &[]);
    sim.run(10);
    let Sim { dm, bridge, physics } = sim;
    drop(physics); // (resources of an app are dropped in no particular order)
    drop(bridge);
    drop(dm);
}

// ---- high speeds ------------------------------------------------------------------------------------

#[test]
fn a_very_fast_part_cannot_pass_through_a_thin_anchored_wall() {
    let mut sim = Sim::new();
    sim.no_gravity();
    let wall = sim.anchored(v(100.0, 0.0, 0.0), v(0.2, 40.0, 40.0));
    let bullet = sim.part(v(0.0, 0.0, 0.0), Vec3::splat(0.5));
    sim.set(bullet, "AssemblyLinearVelocity", Value::V3(v(5000.0, 0.0, 0.0)));
    sim.run(30);
    assert!(sim.pos(bullet).x < 100.0, "stopped by the wall, not through it: {:?}", sim.pos(bullet));
    let _ = wall;
}

#[test]
fn continuous_collision_stops_a_fast_part_against_a_thin_moving_one_and_without_it_the_part_passes() {
    let run = |continuous: bool| {
        let mut sim = Sim::new();
        sim.no_gravity();
        let plate = sim.part(v(100.0, 0.0, 0.0), v(0.2, 40.0, 40.0));
        let bullet = sim.part(v(0.0, 0.0, 0.0), Vec3::splat(0.5));
        sim.flag(bullet, "ContinuousCollision", continuous);
        sim.set(bullet, "AssemblyLinearVelocity", Value::V3(v(6000.0, 0.0, 0.0)));
        sim.run(30);
        (sim.pos(bullet).x, sim.pos(plate).x)
    };
    let (with_bullet, plate) = run(true);
    assert!(with_bullet < plate + 5.0, "it hit the plate and pushed it on: bullet {with_bullet}, plate {plate}");
    let (without, _) = run(false);
    assert!(without > 1000.0, "without it the part tunnels through: {without}");
}

// ---- invalid configuration ---------------------------------------------------------------------------

#[test]
fn values_that_would_break_the_simulation_are_refused_or_held_in_range() {
    let mut sim = Sim::new();
    sim.ground();
    let part = sim.part(v(0.0, 5.0, 0.0), Vec3::ONE);
    for (prop, value) in [
        ("Density", Value::Num(f32::NAN)),
        ("Friction", Value::Num(f32::INFINITY)),
        ("GravityScale", Value::Num(f32::NAN)),
        ("AssemblyLinearVelocity", Value::V3(v(f32::NAN, 0.0, 0.0))),
        ("AssemblyAngularVelocity", Value::V3(v(0.0, f32::INFINITY, 0.0))),
        ("Position", Value::V3(v(f32::INFINITY, 0.0, 0.0))),
        ("Size", Value::V3(v(f32::NAN, 1.0, 1.0))),
        ("CollisionGroup", Value::Str("bad\nname".into())),
        ("CollisionGroup", Value::Str(String::new())),
        ("Density", Value::Bool(true)),
    ] {
        assert!(sim.dm.set_prop(part, prop, value.clone()).is_err(), "{prop} = {value:?} must be refused");
    }
    // Numbers out of range are held in range.
    sim.set(part, "Density", Value::Num(-5.0));
    sim.set(part, "Friction", Value::Num(1000.0));
    sim.set(part, "Elasticity", Value::Num(7.0));
    sim.set(part, "LinearDamping", Value::Num(-3.0));
    sim.set(part, "GravityScale", Value::Num(1.0e9));
    assert_eq!(sim.dm.num(part, "Density", 0.0), 0.001);
    assert_eq!(sim.dm.num(part, "Friction", 0.0), 2.0);
    assert_eq!(sim.dm.num(part, "Elasticity", 0.0), 1.0);
    assert_eq!(sim.dm.num(part, "LinearDamping", 9.0), 0.0);
    assert_eq!(sim.dm.num(part, "GravityScale", 0.0), 100.0);
    sim.run(60);
    assert!(sim.pos(part).is_finite() && sim.vel(part).is_finite());
}

#[test]
fn odd_but_legal_parts_are_simulated_safely() {
    let mut sim = Sim::new();
    sim.ground();
    let tiny = sim.part(v(0.0, 5.0, 0.0), Vec3::splat(0.001));
    let huge = sim.anchored(v(0.0, -60000.0, 0.0), Vec3::splat(100_000.0));
    let flat = sim.part(v(10.0, 5.0, 0.0), v(50.0, 0.001, 50.0));
    let far = sim.part(v(1.0e9, 5.0, 0.0), Vec3::ONE);
    let zero_rotation = sim.part(v(-10.0, 5.0, 0.0), Vec3::ONE);
    sim.set(zero_rotation, "CFrame", Value::CF(v(-10.0, 5.0, 0.0), Quat::from_xyzw(0.0, 0.0, 0.0, 0.0)));
    let wedge = sim.part(v(20.0, 5.0, 0.0), v(0.001, 0.001, 0.001));
    sim.set(wedge, "Shape", Value::Enum("Wedge".into()));
    let ball = sim.part(v(30.0, 5.0, 0.0), v(0.001, 4.0, 0.001));
    sim.set(ball, "Shape", Value::Enum("Ball".into()));
    sim.run(120);
    for id in [tiny, flat, zero_rotation, wedge, ball] {
        assert!(sim.pos(id).is_finite(), "{:?}", sim.pos(id));
    }
    assert!(sim.pos(tiny).y < 1.0, "the tiny part fell to the floor: {}", sim.pos(tiny).y);
    assert!(!sim.bridge.parts.contains_key(&far), "a part a billion studs away is outside the world: not simulated");
    assert_eq!(sim.pos(far).x, 1.0e9, "and it is left alone");
    assert!(sim.bridge.parts.contains_key(&huge));
}

#[test]
fn a_collision_group_that_is_not_registered_behaves_as_default() {
    let mut sim = Sim::new();
    sim.ground();
    let part = sim.part(v(0.0, 5.0, 0.0), Vec3::ONE);
    sim.set(part, "CollisionGroup", Value::Str("NeverRegistered".into()));
    sim.run_until(600, |s| s.sleeping(part));
    assert!(near(sim.pos(part).y, 0.5, 0.05), "it rests on the floor as a Default part would");
}

// ---- who owns the transform ------------------------------------------------------------------------

#[test]
fn the_data_model_shows_the_simulation_and_the_simulation_does_not_echo_it_back() {
    let mut sim = Sim::new();
    sim.ground();
    let part = sim.part(v(0.0, 20.0, 3.0), v(2.0, 1.0, 2.0));
    sim.set(part, "AssemblyAngularVelocity", Value::V3(v(1.0, 2.0, 0.5)));
    for _ in 0..90 {
        sim.step();
        assert_eq!(sim.dm.phys.pending(), 0, "what the simulation writes is never noted as an edit");
        let rec = &sim.bridge.parts[&part];
        let body = rec.body.position();
        assert!((sim.pos(part) - v(body.x, body.y, body.z)).length() < 1e-4, "the data model and the world agree");
    }
}

#[test]
fn setting_the_position_of_a_dynamic_part_teleports_it_and_keeps_its_velocity() {
    let mut sim = Sim::new();
    sim.no_gravity();
    let part = sim.part(v(0.0, 50.0, 0.0), Vec3::ONE);
    sim.set(part, "AssemblyLinearVelocity", Value::V3(v(10.0, 0.0, 0.0)));
    sim.run(30);
    sim.set(part, "Position", Value::V3(v(0.0, 80.0, 0.0)));
    sim.run(1);
    assert!(near(sim.pos(part).y, 80.0, 0.01), "{:?}", sim.pos(part));
    assert!(near(sim.vel(part).x, 10.0, 0.1), "the velocity went on: {:?}", sim.vel(part));
    // Writing the very same pose again is nothing: it does not disturb the part.
    for _ in 0..10 {
        let (p, q) = sim.dm.cframe(part).unwrap();
        sim.dm.set_cframe(part, p, q);
        sim.step();
    }
    assert!(near(sim.vel(part).x, 10.0, 0.1));
}

#[test]
fn a_part_that_is_set_to_a_velocity_keeps_it_and_the_property_follows_the_world() {
    let mut sim = Sim::new();
    sim.no_gravity();
    let part = sim.part(v(0.0, 50.0, 0.0), Vec3::ONE);
    sim.set(part, "AssemblyLinearVelocity", Value::V3(v(0.0, 0.0, -7.0)));
    sim.run(30);
    assert!(near(sim.pos(part).z, -3.5, 0.1), "{:?}", sim.pos(part));
    sim.set(part, "AssemblyLinearVelocity", Value::V3(Vec3::ZERO));
    sim.run(2);
    let z = sim.pos(part).z;
    sim.run(30);
    assert!(near(sim.pos(part).z, z, 0.01), "set to zero: it stopped");
}

#[test]
fn an_anchored_part_ignores_a_velocity_and_says_so() {
    let mut sim = Sim::new();
    let part = sim.anchored(v(0.0, 50.0, 0.0), Vec3::ONE);
    sim.set(part, "AssemblyLinearVelocity", Value::V3(v(5.0, 0.0, 0.0)));
    sim.run(30);
    assert_eq!(sim.pos(part), v(0.0, 50.0, 0.0));
    assert_eq!(sim.vel(part), Vec3::ZERO, "the property reads back what the part does: nothing");
}

#[test]
fn a_kinematic_part_moves_by_its_velocity_and_pushes_what_it_meets_without_being_pushed() {
    let mut sim = Sim::new();
    sim.ground();
    let plow = sim.anchored(v(-10.0, 2.0, 0.0), v(1.0, 4.0, 10.0));
    sim.flag(plow, "Kinematic", true);
    let crate_ = sim.part(v(0.0, 1.0, 0.0), v(2.0, 2.0, 2.0));
    sim.set(plow, "AssemblyLinearVelocity", Value::V3(v(8.0, 0.0, 0.0)));
    sim.run(180);
    assert!(sim.pos(plow).x > 10.0, "it kept going: {:?}", sim.pos(plow));
    assert!(near(sim.vel(plow).x, 8.0, 0.2), "at its own speed, however much it pushed: {:?}", sim.vel(plow));
    assert!(sim.pos(crate_).x > 5.0, "and pushed the crate ahead of it: {:?}", sim.pos(crate_));
}

#[test]
fn setting_the_cframe_of_a_kinematic_part_moves_it_toward_the_target_and_pushes() {
    let mut sim = Sim::new();
    sim.no_gravity();
    let mover = sim.anchored(v(0.0, 10.0, 0.0), Vec3::splat(2.0));
    sim.flag(mover, "Kinematic", true);
    let thing = sim.part(v(2.5, 10.0, 0.0), Vec3::splat(2.0));
    sim.run(2);
    for i in 1..=30 {
        sim.set(mover, "Position", Value::V3(v(i as f32 * 0.5, 10.0, 0.0)));
        sim.step();
    }
    assert!(near(sim.pos(mover).x, 15.0, 0.3), "{:?}", sim.pos(mover));
    assert!(sim.pos(thing).x > 8.0, "pushed along: {:?}", sim.pos(thing));
}

// ---- collision groups -------------------------------------------------------------------------------

#[test]
fn parts_in_groups_that_do_not_collide_pass_through_each_other_and_a_change_takes_effect_at_once() {
    let mut sim = Sim::new();
    let ground = sim.ground();
    let ghost = sim.part(v(0.0, 5.0, 0.0), Vec3::ONE);
    let solid = sim.part(v(10.0, 5.0, 0.0), Vec3::ONE);
    let service = sim.dm.service("PhysicsService").unwrap();
    sim.run_until(600, |s| s.sleeping(ghost) && s.sleeping(solid));
    assert!(near(sim.pos(ghost).y, 0.5, 0.05));

    // Register a group that does not collide with Default, while the game runs, and put the resting part in it.
    let mut groups = groups::CollisionGroups::default();
    groups.register("Ghosts").unwrap();
    groups.set_collidable("Ghosts", "Default", false).unwrap();
    sim.set(service, "CollisionGroups", Value::Str(groups.encode()));
    sim.set(ghost, "CollisionGroup", Value::Str("Ghosts".into()));
    sim.run(40);
    assert!(sim.pos(ghost).y < -5.0, "it fell through the floor: {}", sim.pos(ghost).y);
    assert!(near(sim.pos(solid).y, 0.5, 0.05), "the Default part still rests on it");
    let _ = ground;

    // Allow the collision again: the groups are saved as text and re-read, so the rule applies to parts already in the world.
    let mut back = groups.clone();
    back.set_collidable("Ghosts", "Default", true).unwrap();
    sim.set(service, "CollisionGroups", Value::Str(back.encode()));
    sim.set(ghost, "Position", Value::V3(v(0.0, 8.0, 0.0)));
    sim.set(ghost, "AssemblyLinearVelocity", Value::V3(Vec3::ZERO));
    sim.run_until(600, |s| s.sleeping(ghost));
    assert!(near(sim.pos(ghost).y, 0.5, 0.1), "collides again: {}", sim.pos(ghost).y);
}

// ---- sleeping ---------------------------------------------------------------------------------------

#[test]
fn parts_sleep_when_still_wake_when_touched_pushed_or_told_and_can_be_kept_awake() {
    let mut sim = Sim::new();
    sim.ground();
    let part = sim.part(v(0.0, 3.0, 0.0), Vec3::ONE);
    let insomniac = sim.part(v(10.0, 3.0, 0.0), Vec3::ONE);
    sim.flag(insomniac, "CanSleep", false);
    sim.run_until(600, |s| s.sleeping(part));
    sim.run(120);
    assert!(sim.sleeping(part) && !sim.sleeping(insomniac), "a part with CanSleep off never rests");
    let awake_before = sim.stats().bodies[0].2;
    assert_eq!(awake_before, 1);

    sim.in_frame(|b, dm| b.apply_impulse(dm, part, v(0.0, 20.0, 0.0), None)).unwrap();
    assert!(!sim.sleeping(part), "pushed: awake at once");
    sim.run_until(900, |s| s.sleeping(part));
    assert!(sim.sleeping(part));

    // A part that lands on a sleeping one wakes it.
    let dropper = sim.part(v(0.0, 6.0, 0.0), Vec3::ONE);
    sim.run_until(300, |s| !s.sleeping(part));
    assert!(!sim.sleeping(part) || sim.pos(dropper).y < 3.0);
    sim.run_until(900, |s| s.sleeping(part) && s.sleeping(dropper));

    // Told to: Sleep() and WakeUp().
    sim.in_frame(|b, dm| b.set_asleep(dm, part, false)).unwrap();
    assert!(!sim.sleeping(part));
    sim.in_frame(|b, dm| b.set_asleep(dm, part, true)).unwrap();
    assert!(sim.sleeping(part));
    let ground_err = sim.in_frame(|b, dm| {
        let g = dm.find_first_child(dm.workspace, "Part", false).unwrap();
        b.set_asleep(dm, g, true)
    });
    assert!(ground_err.is_err(), "an anchored part is never asleep or awake");
}

#[test]
fn turning_sleeping_off_in_the_workspace_keeps_everything_awake() {
    let mut sim = Sim::new();
    sim.ground();
    let part = sim.part(v(0.0, 3.0, 0.0), Vec3::ONE);
    let ws = sim.dm.workspace;
    sim.set(ws, "AllowSleeping", Value::Bool(false));
    sim.run(400);
    assert!(!sim.sleeping(part), "nothing sleeps");
    sim.set(ws, "AllowSleeping", Value::Bool(true));
    sim.run_until(600, |s| s.sleeping(part));
    assert!(sim.sleeping(part));
}

// ---- bodies that cannot turn (what a scripted player is made of) ---------------------------------------------

#[test]
fn a_part_with_fixed_rotation_cannot_be_tipped_over_and_is_stopped_by_a_wall() {
    let mut sim = Sim::new();
    sim.ground();
    let body = sim.part(v(0.0, 3.0, 0.0), v(2.0, 5.0, 1.0));
    sim.flag(body, "FixedRotation", true);
    sim.anchored(v(20.0, 5.0, 0.0), v(1.0, 10.0, 20.0));
    sim.run(30);
    sim.in_frame(|b, dm| b.apply_impulse(dm, body, v(0.0, 0.0, 200.0), Some(v(0.0, 2.0, 0.0)))).unwrap();
    sim.run(30);
    let spin = sim.dm.vec3(body, "AssemblyAngularVelocity");
    assert!(spin.length() < 1e-3, "its rotation is locked: {spin:?}");
    // A script walks it by setting its velocity every step.
    for _ in 0..180 {
        let y = sim.dm.vec3(body, "AssemblyLinearVelocity").y;
        sim.set(body, "AssemblyLinearVelocity", Value::V3(v(16.0, y, 0.0)));
        sim.step();
    }
    assert!(sim.pos(body).x < 19.0, "the wall stops it: {:?}", sim.pos(body));
    assert!(sim.dm.cframe(body).unwrap().1.mul_vec3(Vec3::Y).y > 0.999, "and it stayed upright");
}

#[test]
fn fixed_rotation_can_be_switched_off_while_the_game_runs_and_the_part_then_tips() {
    let mut sim = Sim::new();
    sim.ground();
    let body = sim.part(v(0.0, 3.0, 0.0), v(1.0, 5.0, 1.0));
    sim.flag(body, "FixedRotation", true);
    sim.run(30);
    sim.in_frame(|b, dm| b.apply_impulse(dm, body, v(0.0, 0.0, 100.0), Some(v(0.0, 2.0, 0.0)))).unwrap();
    sim.run(10);
    assert!(sim.dm.vec3(body, "AssemblyAngularVelocity").length() < 1e-3);
    sim.flag(body, "FixedRotation", false);
    sim.in_frame(|b, dm| b.apply_impulse(dm, body, v(0.0, 0.0, 300.0), Some(v(0.0, 2.0, 0.0)))).unwrap();
    sim.run(20);
    assert!(sim.dm.cframe(body).unwrap().1.mul_vec3(Vec3::Y).y < 0.999, "free to turn again");
}
// ---- determinism ------------------------------------------------------------------------------------

fn scene(sim: &mut Sim, with_ball: bool) -> Vec<Id> {
    sim.ground();
    let mut ids = Vec::new();
    for layer in 0..6 {
        for i in 0..5 {
            let id = sim.part(v(i as f32 * 2.2 + layer as f32 * 0.1, 0.5 + layer as f32 * 1.1, (layer % 2) as f32 * 0.3), Vec3::ONE);
            ids.push(id);
        }
    }
    if with_ball {
        let ball = sim.part(v(5.0, 20.0, 0.0), Vec3::splat(2.0));
        sim.set(ball, "Shape", Value::Enum("Ball".into()));
        ids.push(ball);
    }
    ids
}

#[test]
fn two_runs_of_the_same_scene_give_the_same_result_to_the_last_bit() {
    let run = || {
        let mut sim = Sim::new();
        let ids = scene(&mut sim, true);
        sim.run(300);
        ids.iter().map(|i| (sim.pos(*i).to_array(), sim.dm.cframe(*i).unwrap().1.to_array())).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn a_resting_stack_stays_put_and_costs_nothing() {
    let mut sim = Sim::new();
    let ids = scene(&mut sim, false);
    sim.run_until(1200, |s| ids.iter().all(|i| s.sleeping(*i)));
    assert!(ids.iter().all(|i| sim.sleeping(*i)), "everything went to sleep");
    let before: Vec<Vec3> = ids.iter().map(|i| sim.pos(*i)).collect();
    sim.run(60);
    assert_eq!(before, ids.iter().map(|i| sim.pos(*i)).collect::<Vec<_>>());
    assert_eq!(sim.stats().movers, 0, "no body moved, so nothing was written back");
}
#[test]
fn an_anchored_zone_reports_the_anchored_parts_inside_it_and_zones_report_each_other() {
    let mut sim = Sim::new();
    let solid = sim.anchored(v(0.0, 0.0, 0.0), Vec3::splat(4.0));
    let zone = sim.anchored(v(1.0, 0.0, 0.0), Vec3::splat(4.0));
    sim.flag(zone, "CanCollide", false);
    sim.dm.watch_touch(zone);
    sim.dm.watch_touch(solid);
    sim.run(10);
    let seen = sim.events();
    assert_eq!(Sim::touched(&seen, zone, solid), 1, "{seen:?}");
    assert_eq!(Sim::touched(&seen, solid, zone), 1);

    // Two parts that do not collide report each other too (Box3D's sensors see sensors).
    let other_zone = sim.anchored(v(1.0, 0.0, 1.0), Vec3::splat(4.0));
    sim.flag(other_zone, "CanCollide", false);
    sim.dm.watch_touch(other_zone);
    sim.run(10);
    let seen = sim.events();
    assert_eq!(Sim::touched(&seen, other_zone, zone), 1, "{seen:?}");
    assert_eq!(Sim::touched(&seen, zone, other_zone), 1);
}
