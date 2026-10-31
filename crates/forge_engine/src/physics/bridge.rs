//! The bridge between the data model and the Box3D world.
//!
//! # Who owns what
//!
//! The data model is the scene: every part has a `CFrame`, a `Size`, physical properties. The Box3D world is a
//! *mirror* of the parts that can be simulated, kept in step by this bridge, and the two never fight over a value:
//!
//! * **Edits go in.** A script or the editor writes a property; the data model notes the instance in its
//!   [`PhysFeed`](crate::dm::PhysFeed). Before the next step [`PhysicsBridge::sync`] reads each noted instance,
//!   compares what it wants with what the world has (the *applied* state kept for every part) and changes the
//!   world only where they differ: nothing is polled, a part nobody touched costs nothing.
//! * **Results come out.** After a step [`PhysicsBridge::post_step`] writes the positions and velocities of the
//!   bodies that moved back into the data model, with the feed *muted*, so the write is not mistaken for an edit
//!   and sent back. The applied state is updated with the same values, which is how the next `sync` can tell a
//!   script's teleport (the data model differs from the applied pose) from the simulation's own motion (it does not).
//! * **Who is authoritative** depends on the body: an anchored part is moved only by edits (the data model leads, a
//!   teleport at most); a kinematic part moves by its velocity, or toward a `CFrame` a script sets (so it pushes
//!   what it meets); a dynamic part is moved by the simulation, and an edit to its `CFrame` or velocity is a
//!   teleport the simulation continues from.
//!
//! Everything the world holds for a part (the body and its shape) is owned here and destroyed with the part:
//! dropping a [`PartRec`] destroys both, so a destroyed, re-parented or no longer simulated part leaves nothing
//! behind.
//!
//! # The `'static` handles
//!
//! `box3d`'s bodies and shapes are owning handles that borrow the world (`Body<'world>`), because dropping one
//! destroys it. The bridge has to keep them for as long as the part lives while the world itself is owned by
//! `bevy_box3d`, so their lifetime is extended to `'static` in [`extend_body`] / [`extend_shape`]. This is sound
//! here because a handle is only an id: every Box3D call made through one is made while the world exists (the
//! bridge is only driven with a reference to the world), and dropping one after the world is gone does nothing
//! (`box3d` checks that the id is still valid before it destroys it - the `world_dropped_first` test pins this).

use std::{
    cell::Cell,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::NonNull,
    rc::Rc,
};

use bevy::{
    log::{error, warn},
    math::{Quat, Vec3},
};
use box3d::{Body, BodyCreateOptions, BodyDef, BodyType, Filter, Hull, MotionLocks, Shape, ShapeDef, ShapeQueryHandle, SurfaceMaterial, World};

use super::{
    box3d_error,
    groups::CollisionGroups,
    shapes::{self, Geometry},
    ColliderShape, PhysicsStats,
};
use crate::{
    core::{registry, FastMap, FastSet, Id, Value},
    dm::{DataModel, Event},
};

// ---- conversions between Bevy's math types and Box3D's ----------------------------------------------

pub(crate) fn bv(v: Vec3) -> box3d::Vec3 {
    box3d::Vec3::new(v.x, v.y, v.z)
}

pub(crate) fn fv(v: box3d::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// A unit-length copy of a rotation (a zero or non-finite one is the identity).
fn unit(q: Quat) -> Quat {
    let l = q.length_squared();
    if l.is_finite() && l > 1e-12 {
        q * l.sqrt().recip()
    } else {
        Quat::IDENTITY
    }
}

/// A rotation Box3D accepts: unit length (a zero or non-finite one is the identity).
pub(crate) fn bq(q: Quat) -> box3d::Quat {
    let q = unit(q);
    box3d::Quat::new(box3d::Vec3::new(q.x, q.y, q.z), q.w)
}

pub(crate) fn fq(q: box3d::Quat) -> Quat {
    unit(Quat::from_xyzw(q.v.x, q.v.y, q.v.z, q.s))
}

pub(crate) fn btransform(p: Vec3, q: Quat) -> box3d::Transform {
    box3d::Transform::new(bv(p), bq(q))
}

fn pack_id(id: Id) -> usize {
    ((u64::from(id.uid) << 32) | u64::from(id.slot)) as usize
}

fn unpack_id(user_data: usize) -> Id {
    let bits = user_data as u64;
    Id { slot: bits as u32, uid: (bits >> 32) as u32 }
}

/// Extends the lifetime of a body handle (see the module docs).
///
/// # Safety
/// The handle must only be used while the world it was made in exists, or dropped.
unsafe fn extend_body(body: Body<'_>) -> Body<'static> {
    std::mem::transmute(body)
}

/// Extends the lifetime of a shape handle (see the module docs).
///
/// # Safety
/// As for [`extend_body`]; the shape must also be dropped before its body.
unsafe fn extend_shape(shape: Shape<'_>) -> Shape<'static> {
    std::mem::transmute(shape)
}

// ---- what a part wants -------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BodyKind {
    /// Anchored: never moved by the simulation.
    Static,
    /// Anchored and `Kinematic`: moved by its velocity (or a `CFrame` set toward), and pushes what it meets.
    Kinematic,
    Dynamic,
}

impl BodyKind {
    fn body_type(self) -> BodyType {
        match self {
            BodyKind::Static => BodyType::Static,
            BodyKind::Kinematic => BodyType::Kinematic,
            BodyKind::Dynamic => BodyType::Dynamic,
        }
    }
}

/// Everything the simulation needs to know about a part, read from the data model.
#[derive(Clone, Debug, PartialEq)]
pub struct PartSpec {
    pub pos: Vec3,
    pub rot: Quat,
    pub shape: ColliderShape,
    pub size: Vec3,
    pub kind: BodyKind,
    pub can_collide: bool,
    pub can_touch: bool,
    pub can_query: bool,
    pub density: f32,
    pub friction: f32,
    pub elasticity: f32,
    pub group: String,
    pub gravity_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub bullet: bool,
    pub can_sleep: bool,
    pub lin_vel: Vec3,
    pub ang_vel: Vec3,
    /// The body cannot turn (the FixedRotation property): a script-driven character that must stay upright.
    pub fixed_rotation: bool,
}

/// What a part wants, or `None` when it is not simulated: it is not a part, not in the world, or lies outside the world the simulation can hold.
pub fn read_spec(dm: &DataModel, id: Id) -> Option<PartSpec> {
    if !dm.has_component(id, "Rigidbody") || !dm.in_workspace(id) {
        return None;
    }
    let info = dm.collider(id)?;
    if !shapes::in_world(info.center, info.size) {
        return None;
    }
    let kind = match (info.anchored, dm.flag(id, "Kinematic")) {
        (false, _) => BodyKind::Dynamic,
        (true, false) => BodyKind::Static,
        (true, true) => BodyKind::Kinematic,
    };
    Some(PartSpec {
        pos: info.center,
        rot: info.rotation,
        shape: info.shape,
        size: info.size,
        kind,
        can_collide: info.can_collide,
        can_touch: info.can_touch,
        can_query: dm.flag(id, "CanQuery"),
        density: dm.num(id, "Density", 1.0),
        friction: dm.num(id, "Friction", 0.3),
        elasticity: dm.num(id, "Elasticity", 0.0),
        group: dm.str_ref(id, "CollisionGroup").to_owned(),
        gravity_scale: dm.num(id, "GravityScale", 1.0),
        linear_damping: dm.num(id, "LinearDamping", 0.0),
        angular_damping: dm.num(id, "AngularDamping", 0.0),
        bullet: dm.flag(id, "ContinuousCollision"),
        can_sleep: dm.flag(id, "CanSleep"),
        lin_vel: dm.vec3(id, "AssemblyLinearVelocity"),
        ang_vel: dm.vec3(id, "AssemblyAngularVelocity"),
        fixed_rotation: dm.flag(id, "FixedRotation"),
    })
}

// ---- what the world has -----------------------------------------------------------------------------

/// A part's body and shape in the Box3D world, and what has been applied to them.
pub(crate) struct PartRec {
    // (The shape is declared first: it is dropped, and so destroyed, before its body.)
    shape: Option<Shape<'static>>,
    pub(crate) body: Body<'static>,
    /// What the world was last given, or last told us: the simulation's own writes to the data model are put
    /// here too, so an edit is whatever differs from it.
    pub(crate) applied: PartSpec,
    /// Whether anything listens to `Touched` / `TouchEnded` on this part (contact events are only produced then).
    listens: bool,
    /// Whether the simulation last said the body is asleep.
    asleep: bool,
    /// The category and mask the shape has (they follow the part's group and what the groups say).
    filter: (u64, u64),
}

impl PartRec {
    pub(crate) fn shape_handle(&self) -> Option<ShapeQueryHandle> {
        self.shape.as_ref().map(|s| s.id().handle())
    }
}

/// The time step an edit made outside a step uses (a kinematic body is moved toward a target in one step).
const DEFAULT_STEP: f32 = 1.0 / 60.0;

#[derive(Clone, Copy, PartialEq)]
struct WorldSettings {
    gravity: f32,
    sleeping: bool,
    continuous: bool,
}

pub struct PhysicsBridge {
    pub(crate) parts: FastMap<Id, PartRec>,
    /// Which part a shape belongs to (for contact events and query hits).
    pub(crate) by_shape: FastMap<ShapeQueryHandle, Id>,
    /// Shapes destroyed since the last step's events were read: their end-touch events arrive one step later.
    graveyard: Vec<ShapeQueryHandle>,
    pub(crate) joints: super::joints::Joints,
    pub(crate) groups: CollisionGroups,
    groups_text: Option<String>,
    warned_groups: FastSet<String>,
    settings: Option<WorldSettings>,
    /// Pairs of parts that are touching and that something listens to.
    touching: FastSet<(Id, Id)>,
    /// Where the world is put while a frame of scripts runs (see [`WorldScope`]).
    slot: WorldSlot,
    dt: f32,
    steps_pending: u32,
    pub stats: PhysicsStats,
}

impl Default for PhysicsBridge {
    fn default() -> Self {
        PhysicsBridge {
            parts: FastMap::default(),
            by_shape: FastMap::default(),
            graveyard: Vec::new(),
            joints: Default::default(),
            groups: CollisionGroups::default(),
            groups_text: None,
            warned_groups: FastSet::default(),
            settings: None,
            touching: FastSet::default(),
            slot: Rc::new(Cell::new(None)),
            dt: DEFAULT_STEP,
            steps_pending: 0,
            stats: PhysicsStats::default(),
        }
    }
}

impl PhysicsBridge {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- attaching and the world a frame of scripts sees --------------------------------------------

    /// Starts listening to the data model: from now on it records what changes, and everything that is in it
    /// already is looked at by the next [`sync`](Self::sync).
    pub fn attach(&mut self, dm: &mut DataModel) {
        dm.phys.enabled = true;
        for id in dm.all_ids() {
            dm.phys_mark(id);
        }
    }

    /// The body of a part that the simulation moves (not anchored), for the movers that push parts around.
    pub fn mover_body(&self, id: Id) -> Option<&Body<'static>> {
        self.parts.get(&id).filter(|rec| rec.applied.kind == BodyKind::Dynamic).map(|rec| &rec.body)
    }

    /// The place a frame of scripts is given the world in: put it there with [`WorldScope::enter`].
    pub fn slot(&self) -> WorldSlot {
        self.slot.clone()
    }

    pub(crate) fn world<'a>(&self) -> Result<&'a World, String> {
        match self.slot.get() {
            // SAFETY: the pointer was made from a live `&World` by `WorldScope::enter`, and the guard clears it
            // before that reference's borrow can end. Nothing keeps the result past the call that asked for it.
            Some(p) => Ok(unsafe { p.as_ref() }),
            None => Err("the physics world is not running (this only works while the game is playing)".into()),
        }
    }

    // ---- bringing the world up to date ----------------------------------------------------------------

    /// Applies everything recorded in the data model since the last call to the world.
    pub fn sync(&mut self, dm: &mut DataModel, world: &World) {
        self.apply_groups(dm);
        self.apply_world_settings(dm, world);
        let mut parts: Vec<Id> = Vec::new();
        let mut joints: Vec<Id> = Vec::new();
        let mut seen: FastSet<Id> = FastSet::default();
        for id in dm.phys.take() {
            if !seen.insert(id) {
                continue;
            }
            match dm.get(id) {
                // Gone: it may have been a part or a constraint, and either way what it held is released.
                None => {
                    parts.push(id);
                    joints.push(id);
                    // (It may have been an attachment a constraint was joined by.)
                    joints.extend(self.joints.users_of(id));
                }
                Some(inst) if inst.class.has_component("Rigidbody") => parts.push(id),
                // An attachment moved, was re-parented or changed: the constraints joined by it are looked at again.
                Some(inst) if inst.class.is_a("Attachment") => joints.extend(self.joints.users_of(id)),
                Some(inst) if inst.class.has_component("Joint") => joints.push(id),
                Some(_) => {}
            }
        }
        for id in parts {
            self.reconcile_part(dm, world, id);
        }
        // Constraints come after the parts they connect; so do the ones whose parts were just rebuilt or removed.
        for id in self.joints.take_rechecks() {
            if seen.insert(id) {
                joints.push(id);
            }
        }
        for id in joints {
            self.reconcile_joint(dm, world, id);
        }
    }

    /// Brings the world up to date from inside a frame of scripts, so a part created a moment ago can be pushed,
    /// asked its mass or hit by a ray in the same frame.
    pub(crate) fn flush(&mut self, dm: &mut DataModel) -> Result<(), String> {
        let world = self.world()?;
        self.sync(dm, world);
        Ok(())
    }

    fn apply_world_settings(&mut self, dm: &DataModel, world: &World) {
        let ws = dm.workspace;
        let wanted = WorldSettings { gravity: dm.num(ws, "Gravity", 196.2), sleeping: dm.flag(ws, "AllowSleeping"), continuous: dm.flag(ws, "ContinuousCollision") };
        if self.settings != Some(wanted) {
            world.set_gravity(box3d::Vec3::new(0.0, -wanted.gravity, 0.0));
            world.set_sleeping_enabled(wanted.sleeping);
            world.set_continuous_enabled(wanted.continuous);
            self.settings = Some(wanted);
        }
    }

    /// Reads the collision groups saved with the scene; when they changed, every part is filtered again.
    fn apply_groups(&mut self, dm: &mut DataModel) {
        let Some(service) = dm.service("PhysicsService") else { return };
        let text = dm.str_ref(service, "CollisionGroups");
        if self.groups_text.as_deref() == Some(text) {
            return;
        }
        let text = text.to_owned();
        let (groups, problems) = CollisionGroups::decode(&text);
        for p in problems {
            warn!("[physics] collision groups: {p}");
        }
        self.groups = groups;
        self.groups_text = Some(text);
        // Every part's category and mask depend on the groups.
        let ids: Vec<Id> = self.parts.keys().copied().collect();
        for id in ids {
            dm.phys.mark_internal(id);
        }
    }

    fn warn_group(&mut self, name: &str) {
        if self.warned_groups.insert(name.to_owned()) {
            warn!("[physics] '{name}' is not a registered collision group; parts in it collide as '{}' (register it with PhysicsService:RegisterCollisionGroup)", super::groups::DEFAULT_GROUP);
        }
    }

    fn reconcile_part(&mut self, dm: &mut DataModel, world: &World, id: Id) {
        let wanted = read_spec(dm, id);
        match (wanted, self.parts.contains_key(&id)) {
            (None, false) => {}
            (None, true) => self.remove_part(id),
            (Some(spec), false) => self.create_part(dm, world, id, spec),
            (Some(spec), true) => self.update_part(dm, id, spec),
        }
    }

    fn remove_part(&mut self, id: Id) {
        let Some(rec) = self.parts.remove(&id) else { return };
        if let Some(handle) = rec.shape_handle() {
            // (Its end-touch events come one step later and still have to find their part.)
            self.graveyard.push(handle);
        }
        drop(rec);
        self.joints.part_gone(id);
    }

    fn shape_def(&mut self, spec: &PartSpec, listens: bool) -> ShapeDef {
        let (category, mask, known) = self.groups.filter_for(&spec.group);
        if !known {
            self.warn_group(&spec.group);
        }
        let mut def = ShapeDef {
            density: spec.density,
            friction: spec.friction,
            surface_material: Some(SurfaceMaterial { friction: spec.friction, restitution: spec.elasticity, ..SurfaceMaterial::default() }),
            filter: Filter { category_bits: category, mask_bits: mask, group_index: 0 },
            is_sensor: !spec.can_collide,
            ..ShapeDef::default()
        };
        // A part is a *visitor* of every sensor when it can be touched; it produces contact events itself only
        // while something listens, so a part nobody watches costs the simulation nothing extra.
        def.enable_sensor_events = spec.can_touch;
        def.enable_contact_events = spec.can_touch && listens;
        def
    }

    /// Makes the one shape of a part: a solid shape, or (for a part that does not collide) a sensor, which
    /// has no collision response but is still found by queries and reports overlaps.
    fn make_shape(&mut self, body: &Body<'static>, spec: &PartSpec, listens: bool) -> Result<Shape<'static>, String> {
        let def = self.shape_def(spec, listens);
        let made = catch_unwind(AssertUnwindSafe(|| match shapes::geometry(spec.shape, spec.size) {
            Geometry::Box { half } => body.try_create_box(bv(half), def).map_err(box3d_error),
            Geometry::Sphere { radius } => Ok(body.create_sphere(box3d::Vec3::ZERO, radius, def)),
            Geometry::Hull(points) => {
                let points: Vec<box3d::Vec3> = points.into_iter().map(bv).collect();
                let hull = Hull::from_points(&points, 128).map_err(|e| format!("invalid hull: {}", box3d_error(e)))?;
                Ok(body.create_hull(&hull, def))
            }
        }));
        match made {
            // SAFETY: see the module docs; the shape is stored beside (and dropped before) its body.
            Ok(Ok(shape)) => Ok(unsafe { extend_shape(shape) }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Box3D rejected the shape".into()),
        }
    }

    fn create_part(&mut self, dm: &mut DataModel, world: &World, id: Id, spec: PartSpec) {
        let moving = spec.kind != BodyKind::Static;
        let def = BodyDef {
            body_type: spec.kind.body_type(),
            position: bv(spec.pos),
            rotation: bq(spec.rot),
            linear_velocity: if moving { bv(spec.lin_vel) } else { box3d::Vec3::ZERO },
            angular_velocity: if moving { bv(spec.ang_vel) } else { box3d::Vec3::ZERO },
            linear_damping: spec.linear_damping,
            angular_damping: spec.angular_damping,
            sleep_threshold: None,
            user_data: pack_id(id),
        };
        let body = match world.try_create_body_with_options(def, BodyCreateOptions::default()) {
            // SAFETY: see the module docs.
            Ok(b) => unsafe { extend_body(b) },
            Err(e) => return error!("[physics] could not create a body for {}: {}", dm.full_name(id), box3d_error(e)),
        };
        body.set_gravity_scale(spec.gravity_scale);
        body.set_bullet(spec.bullet);
        body.set_sleep_enabled(spec.can_sleep);
        body.set_motion_locks(motion_locks(spec.fixed_rotation));
        let listens = dm.touch_watch.contains(&id);
        let shape = match self.make_shape(&body, &spec, listens) {
            Ok(s) => s,
            Err(e) => return error!("[physics] could not make the shape of {}: {e}", dm.full_name(id)),
        };
        self.by_shape.insert(shape.id().handle(), id);
        if spec.kind == BodyKind::Static && spec.lin_vel != Vec3::ZERO {
            // An anchored part does not move: what was asked of it reads back as what it does.
            write_muted(dm, id, "AssemblyLinearVelocity", Value::V3(Vec3::ZERO));
        }
        let (category, mask, _) = self.groups.filter_for(&spec.group);
        self.parts.insert(id, PartRec { shape: Some(shape), body, applied: spec, listens, asleep: false, filter: (category, mask) });
        self.joints.part_appeared(id);
    }

    fn update_part(&mut self, dm: &mut DataModel, id: Id, spec: PartSpec) {
        let listens = dm.touch_watch.contains(&id);
        let dt = self.dt;
        let rebuild = {
            let rec = self.parts.get(&id).expect("checked by the caller");
            let a = &rec.applied;
            (a.shape, shapes::collision_size(a.size), a.can_collide) != (spec.shape, shapes::collision_size(spec.size), spec.can_collide)
        };
        if rebuild {
            // A new shape (or a solid one made a sensor, or the reverse): the body keeps its place and motion.
            let old = self.parts.get_mut(&id).and_then(|r| r.shape.take());
            if let Some(shape) = old {
                self.by_shape.remove(&shape.id().handle());
                self.graveyard.push(shape.id().handle());
                drop(shape);
            }
            let made = {
                let rec = self.parts.get(&id).expect("checked by the caller");
                // (`make_shape` needs `&mut self` for the group warning, so the body is borrowed by pointer.)
                let body: *const Body<'static> = &rec.body;
                // SAFETY: `body` points into `self.parts`, which `make_shape` does not touch.
                self.make_shape(unsafe { &*body }, &spec, listens)
            };
            match made {
                Ok(shape) => {
                    self.by_shape.insert(shape.id().handle(), id);
                    if let Some(rec) = self.parts.get_mut(&id) {
                        rec.shape = Some(shape);
                    }
                }
                Err(e) => {
                    error!("[physics] could not remake the shape of {}: {e}", dm.full_name(id));
                    self.remove_part(id);
                    return;
                }
            }
        }

        let (category, mask, known) = self.groups.filter_for(&spec.group);
        if !known {
            self.warn_group(&spec.group);
        }
        let Some(rec) = self.parts.get_mut(&id) else { return };
        let a = rec.applied.clone();
        let body = &rec.body;
        // (A new shape wakes the body: something may now be resting on nothing, or inside it.)
        let mut wake = rebuild;

        if spec.kind != a.kind {
            body.set_body_type(spec.kind.body_type());
            wake = true;
        }
        if !rebuild {
            if let Some(shape) = &rec.shape {
                if spec.density != a.density {
                    shape.set_density(spec.density, true);
                }
                if spec.friction != a.friction {
                    shape.set_friction(spec.friction);
                }
                if spec.elasticity != a.elasticity {
                    shape.set_restitution(spec.elasticity);
                }
                if (category, mask) != rec.filter {
                    // (With contacts re-checked: a part resting on something it no longer collides with must fall.)
                    shape.set_filter_with_contact_update(Filter { category_bits: category, mask_bits: mask, group_index: 0 }, true);
                    wake = true;
                }
                if spec.can_touch != a.can_touch {
                    shape.enable_sensor_events(spec.can_touch);
                }
                if (spec.can_touch, listens) != (a.can_touch, rec.listens) {
                    shape.enable_contact_events(spec.can_touch && listens);
                }
            }
        }
        // (A shape that was just made was given the right filter and event flags.)
        rec.filter = (category, mask);
        rec.listens = listens;
        if spec.gravity_scale != a.gravity_scale {
            body.set_gravity_scale(spec.gravity_scale);
            wake = true;
        }
        if spec.linear_damping != a.linear_damping {
            body.set_linear_damping(spec.linear_damping);
        }
        if spec.angular_damping != a.angular_damping {
            body.set_angular_damping(spec.angular_damping);
        }
        if spec.bullet != a.bullet {
            body.set_bullet(spec.bullet);
        }
        if spec.can_sleep != a.can_sleep {
            body.set_sleep_enabled(spec.can_sleep);
            if !spec.can_sleep {
                wake = true;
            }
        }
        if spec.fixed_rotation != a.fixed_rotation {
            body.set_motion_locks(motion_locks(spec.fixed_rotation));
        }
        // An edit to the pose or the velocity (what the simulation wrote back is already in `applied`).
        if (spec.pos, spec.rot) != (a.pos, a.rot) {
            if spec.kind == BodyKind::Kinematic {
                body.set_target_transform(btransform(spec.pos, spec.rot), dt, true);
            } else {
                body.set_transform(bv(spec.pos), bq(spec.rot));
            }
            wake = true;
        }
        if spec.kind != BodyKind::Static {
            if spec.lin_vel != a.lin_vel {
                body.set_linear_velocity(bv(spec.lin_vel));
                wake = true;
            }
            if spec.ang_vel != a.ang_vel {
                body.set_angular_velocity(bv(spec.ang_vel));
                wake = true;
            }
        } else if spec.lin_vel != Vec3::ZERO || spec.ang_vel != Vec3::ZERO {
            write_muted(dm, id, "AssemblyLinearVelocity", Value::V3(Vec3::ZERO));
            write_muted(dm, id, "AssemblyAngularVelocity", Value::V3(Vec3::ZERO));
        }
        let woke = wake && spec.kind != BodyKind::Static;
        if woke {
            body.set_awake(true);
        }
        let was_asleep = woke && std::mem::replace(&mut rec.asleep, false);
        let mut applied = spec;
        if applied.kind == BodyKind::Static {
            applied.lin_vel = Vec3::ZERO;
            applied.ang_vel = Vec3::ZERO;
        }
        rec.applied = applied;
        if was_asleep {
            write_muted(dm, id, "Sleeping", Value::Bool(false));
        }
    }

    // ---- one step -----------------------------------------------------------------------------------------

    /// Everything that happens before the world steps: edits are applied, then the systems components registered
    /// (the physics hooks of components, such as the movers) run.
    pub fn pre_step(&mut self, dm: &mut DataModel, world: &World, dt: f32) {
        self.dt = dt;
        self.steps_pending += 1;
        self.sync(dm, world);
        for (_, _, system) in registry().physics_systems() {
            system(&mut StepCtx { dm, bridge: self, world, dt });
        }
        // What the systems wrote (a mover's velocity) is in the world now, not an edit to come.
        self.sync(dm, world);
    }

    /// Everything that happens after the world stepped: the results go back into the data model, touches are
    /// turned into events, parts that fell out of the world are removed.
    pub fn post_step(&mut self, dm: &mut DataModel, world: &World) {
        let mut moved = 0u32;
        let mut fallen: Vec<Id> = Vec::new();
        let destroy_height = dm.num(dm.workspace, "FallenPartsDestroyHeight", -500.0);
        let was_muted = std::mem::replace(&mut dm.phys.muted, true);
        for event in world.body_events().moves() {
            moved += 1;
            let id = unpack_id(event.user_data);
            let Some(rec) = self.parts.get_mut(&id) else { continue };
            let a = &mut rec.applied;
            if a.kind == BodyKind::Static {
                continue;
            }
            let (pos, rot) = (fv(event.transform.p), fq(event.transform.q));
            if (pos, rot) != (a.pos, a.rot) {
                a.pos = pos;
                a.rot = rot;
                if let Some((p0, q0)) = dm.cframe(id) {
                    if (p0, q0) != (pos, rot) {
                        dm.set_cframe(id, pos, rot);
                    }
                }
            }
            let (lin, ang) = (fv(rec.body.linear_velocity()), fv(rec.body.angular_velocity()));
            if lin != a.lin_vel {
                a.lin_vel = lin;
                write_muted(dm, id, "AssemblyLinearVelocity", Value::V3(lin));
            }
            if ang != a.ang_vel {
                a.ang_vel = ang;
                write_muted(dm, id, "AssemblyAngularVelocity", Value::V3(ang));
            }
            if event.fell_asleep != rec.asleep {
                rec.asleep = event.fell_asleep;
                write_muted(dm, id, "Sleeping", Value::Bool(event.fell_asleep));
            }
            if a.kind == BodyKind::Dynamic && pos.y < destroy_height {
                fallen.push(id);
            }
        }
        dm.phys.muted = was_muted;
        self.emit_touches(dm, world);
        for id in fallen {
            dm.destroy(id);
        }
        self.stats.movers = moved;
        self.stats.touch_pairs = self.touching.len() as u32;
        self.collect_stats(world);
    }

    fn collect_stats(&mut self, world: &World) {
        let (mut statics, mut kinematic, mut dynamic) = (0u32, 0u32, 0u32);
        for rec in self.parts.values() {
            match rec.applied.kind {
                BodyKind::Static => statics += 1,
                BodyKind::Kinematic => kinematic += 1,
                BodyKind::Dynamic => dynamic += 1,
            }
        }
        self.stats.static_boxes = statics;
        self.stats.bodies = vec![("Dynamic", dynamic, world.awake_body_count().max(0) as u32), ("Kinematic", kinematic, kinematic)];
        self.stats.joints = self.joints.count() as u32;
    }

    /// Called once per rendered frame: how many fixed steps the physics ran since the last one.
    pub fn end_frame(&mut self) {
        self.stats.substeps = std::mem::take(&mut self.steps_pending);
    }

    // ---- touches -------------------------------------------------------------------------------------------

    fn emit_touches(&mut self, dm: &mut DataModel, world: &World) {
        let mut produced = 0u32;
        let contacts = world.contact_events();
        for e in contacts.begins() {
            produced += self.touch(dm, e.shape_a.handle(), e.shape_b.handle(), true);
        }
        for e in contacts.ends() {
            produced += self.touch(dm, e.shape_a.handle(), e.shape_b.handle(), false);
        }
        let sensors = world.sensor_events();
        for e in sensors.begins() {
            produced += self.touch(dm, e.sensor.handle(), e.visitor.handle(), true);
        }
        for e in sensors.ends() {
            produced += self.touch(dm, e.sensor.handle(), e.visitor.handle(), false);
        }
        self.stats.touch_events = produced;
        // The events of shapes destroyed last step have been read now.
        for handle in self.graveyard.drain(..) {
            self.by_shape.remove(&handle);
        }
    }

    /// One touch began or ended between two shapes. Returns how many events it produced.
    fn touch(&mut self, dm: &mut DataModel, a: ShapeQueryHandle, b: ShapeQueryHandle, began: bool) -> u32 {
        let (Some(&ia), Some(&ib)) = (self.by_shape.get(&a), self.by_shape.get(&b)) else { return 0 };
        if ia == ib {
            return 0;
        }
        let key = if ia < ib { (ia, ib) } else { (ib, ia) };
        if began {
            // Both parts must be touchable; a pair is reported once however many contacts it has.
            let touchable = |id: &Id| self.parts.get(id).is_some_and(|r| r.applied.can_touch);
            if !(touchable(&ia) && touchable(&ib)) || !self.touching.insert(key) {
                return 0;
            }
        } else if !self.touching.remove(&key) {
            return 0;
        }
        let mut produced = 0;
        for (part, other) in [(ia, ib), (ib, ia)] {
            if dm.touch_watch.contains(&part) && dm.exists(part) {
                dm.events.push(if began { Event::Touched(part, other) } else { Event::TouchEnded(part, other) });
                produced += 1;
            }
        }
        produced
    }

    // ---- what scripts ask ----------------------------------------------------------------------------------

    /// The body of a part, made now if the part was only just created (so scripts can use a part at once).
    pub(crate) fn body_of(&mut self, dm: &mut DataModel, id: Id) -> Result<&Body<'static>, String> {
        if !self.parts.contains_key(&id) {
            self.flush(dm)?;
        }
        self.parts.get(&id).map(|r| &r.body).ok_or_else(|| {
            if !dm.in_workspace(id) {
                format!("{} is not in the workspace, so it has no physics", dm.name(id))
            } else {
                format!("{} is not simulated (it is outside the world)", dm.name(id))
            }
        })
    }

    fn dynamic_body(&mut self, dm: &mut DataModel, id: Id, what: &str) -> Result<&Body<'static>, String> {
        let name = dm.name(id);
        let body = self.body_of(dm, id)?;
        if body.body_type() != BodyType::Dynamic {
            return Err(format!("cannot {what} {name}: it is anchored (unanchor it first)"));
        }
        Ok(body)
    }

    /// An impulse (momentum, studs * mass per second) at the centre of mass, or at a point in the world.
    pub fn apply_impulse(&mut self, dm: &mut DataModel, id: Id, impulse: Vec3, at: Option<Vec3>) -> Result<(), String> {
        check_finite("impulse", impulse)?;
        if let Some(p) = at {
            check_finite("position", p)?;
        }
        let body = self.dynamic_body(dm, id, "apply an impulse to")?;
        match at {
            Some(p) => body.apply_linear_impulse(bv(impulse), bv(p), true),
            None => body.apply_linear_impulse_to_center(bv(impulse), true),
        }
        self.after_change(dm, id);
        Ok(())
    }

    pub fn apply_angular_impulse(&mut self, dm: &mut DataModel, id: Id, impulse: Vec3) -> Result<(), String> {
        check_finite("impulse", impulse)?;
        let body = self.dynamic_body(dm, id, "apply an angular impulse to")?;
        body.apply_angular_impulse(bv(impulse), true);
        self.after_change(dm, id);
        Ok(())
    }

    /// Puts what the world now says about a body (velocity, asleep) into the data model at once, so a script that
    /// pushes a part and reads its velocity in the next line sees the push.
    fn after_change(&mut self, dm: &mut DataModel, id: Id) {
        let Some(rec) = self.parts.get_mut(&id) else { return };
        let (lin, ang) = (fv(rec.body.linear_velocity()), fv(rec.body.angular_velocity()));
        rec.applied.lin_vel = lin;
        rec.applied.ang_vel = ang;
        rec.asleep = false;
        let was = std::mem::replace(&mut dm.phys.muted, true);
        write_muted(dm, id, "AssemblyLinearVelocity", Value::V3(lin));
        write_muted(dm, id, "AssemblyAngularVelocity", Value::V3(ang));
        write_muted(dm, id, "Sleeping", Value::Bool(false));
        dm.phys.muted = was;
    }

    /// The mass of a part: the world's, when it has a body for it; else what its size and density give.
    pub fn mass(&mut self, dm: &mut DataModel, id: Id) -> f32 {
        let _ = self.flush(dm);
        match self.parts.get(&id) {
            Some(rec) if rec.applied.kind == BodyKind::Dynamic => rec.body.mass(),
            _ => analytic_mass(dm, id),
        }
    }

    /// How fast a point of a part (given in the world) is moving: the part's velocity plus its turning.
    pub fn velocity_at(&mut self, dm: &mut DataModel, id: Id, point: Vec3) -> Result<Vec3, String> {
        check_finite("position", point)?;
        let _ = self.flush(dm);
        if let Some(rec) = self.parts.get(&id) {
            return Ok(fv(rec.body.world_point_velocity(bv(point))));
        }
        // No body (the part is not in the world): the same arithmetic on the data model's values.
        let center = dm.cframe(id).map_or(Vec3::ZERO, |c| c.0);
        Ok(dm.vec3(id, "AssemblyLinearVelocity") + dm.vec3(id, "AssemblyAngularVelocity").cross(point - center))
    }

    pub fn set_asleep(&mut self, dm: &mut DataModel, id: Id, asleep: bool) -> Result<(), String> {
        let name = dm.name(id);
        let body = self.body_of(dm, id)?;
        if body.body_type() == BodyType::Static {
            return Err(format!("{name} is anchored: it is never asleep or awake"));
        }
        body.set_awake(!asleep);
        if let Some(rec) = self.parts.get_mut(&id) {
            rec.asleep = asleep;
        }
        let was = std::mem::replace(&mut dm.phys.muted, true);
        write_muted(dm, id, "Sleeping", Value::Bool(asleep));
        dm.phys.muted = was;
        Ok(())
    }

    /// How many parts the world holds a body for.
    pub fn body_count(&self) -> usize {
        self.parts.len()
    }

    /// The Box3D shape of a part (for tests and tools that look at the world directly).
    pub fn shape_of(&self, id: Id) -> Option<ShapeQueryHandle> {
        self.parts.get(&id).and_then(PartRec::shape_handle)
    }

    /// The part a Box3D shape belongs to.
    pub fn part_of(&self, shape: ShapeQueryHandle) -> Option<Id> {
        self.by_shape.get(&shape).copied()
    }

    /// Lets go of everything (a new scene was loaded): the bodies and shapes are destroyed.
    pub fn reset(&mut self) {
        self.joints.clear();
        self.parts.clear();
        self.by_shape.clear();
        self.graveyard.clear();
        self.touching.clear();
        self.groups_text = None;
        self.settings = None;
    }
}

impl Drop for PhysicsBridge {
    fn drop(&mut self) {
        // Joints before the bodies they connect.
        self.joints.clear();
        self.parts.clear();
    }
}

/// Where the world is made available to the methods scripts call.
pub type WorldSlot = Rc<Cell<Option<NonNull<World>>>>;

/// While this lives, the world is reachable through the bridge the slot belongs to (queries, impulses). The pointer
/// is only dereferenced by the bridge's accessor, which fails outside a scope.
pub struct WorldScope {
    slot: WorldSlot,
}

impl WorldScope {
    pub fn enter(slot: WorldSlot, world: &World) -> WorldScope {
        slot.set(Some(NonNull::from(world)));
        WorldScope { slot }
    }
}

impl Drop for WorldScope {
    fn drop(&mut self) {
        self.slot.set(None);
    }
}

/// What a system a component registered for the physics step gets.
pub struct StepCtx<'a> {
    pub dm: &'a mut DataModel,
    pub bridge: &'a mut PhysicsBridge,
    pub world: &'a World,
    /// The length of the step, in seconds.
    pub dt: f32,
}

fn motion_locks(fixed_rotation: bool) -> MotionLocks {
    MotionLocks { angular_x: fixed_rotation, angular_y: fixed_rotation, angular_z: fixed_rotation, ..MotionLocks::default() }
}

/// Writes a value the simulation produced (the caller has the feed muted, or this mutes it for the write).
pub(crate) fn write_muted(dm: &mut DataModel, id: Id, name: &str, value: Value) {
    let was = std::mem::replace(&mut dm.phys.muted, true);
    let _ = dm.set_prop(id, name, value);
    dm.phys.muted = was;
}

pub(crate) fn check_finite(what: &str, v: Vec3) -> Result<(), String> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(format!("the {what} must be a finite Vector3 (got NaN or infinity)"))
    }
}

/// The mass a part has by its size and density.
pub fn analytic_mass(dm: &DataModel, id: Id) -> f32 {
    match dm.collider(id) {
        Some(info) => shapes::volume(info.shape, info.size) * dm.num(id, "Density", 1.0),
        None => 0.0,
    }
}
