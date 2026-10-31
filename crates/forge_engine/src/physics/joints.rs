//! Constraints: the joints between parts.
//!
//! A constraint is an instance (`WeldConstraint`, `HingeConstraint`, ...) with two parts and the frames it joins
//! them at. The bridge keeps one Box3D joint for each constraint that is *active*: enabled, in the workspace, and
//! with both of its parts simulated (a missing `Part1` joins `Part0` to the world). It is read like a part is: a
//! changed property marks the constraint, [`PhysicsBridge::reconcile_joint`] builds what it should be now and
//! compares it with what is there. Anything that differs rebuilds the joint (Box3D cannot change a joint's frames
//! or kind, and rebuilding a joint is cheap), anything that is the same leaves it alone.
//!
//! The frames `C0` and `C1` are CFrames relative to `Part0` and `Part1`. The joint holds the two frames together:
//! a hinge turns about their X axis, a prismatic constraint slides along it, a ball socket twists about it. (Box3D's
//! own revolute and spherical joints use the Z axis; the bridge turns the frames to make X the axis everywhere.)
//! A weld holds the parts as they are when it is made: it has no frames of its own.

use std::any::Any;

use bevy::{
    log::error,
    math::{Quat, Vec3},
};
use box3d::{Body, DistanceJointDef, PrismaticJointDef, RevoluteJointDef, SphericalJointDef, WeldJointDef, World};

use super::{box3d_error, bridge::btransform, PhysicsBridge};
use crate::{
    core::{FastMap, Id},
    dm::DataModel,
};

/// The smallest length a distance joint is given (Box3D clamps it as well).
const MIN_LENGTH: f32 = 0.01;
/// Box3D limits revolute and twist angles to just under a half turn either way.
const MAX_ANGLE: f32 = 0.99 * std::f32::consts::PI;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DistanceMode {
    /// Keeps the two frames exactly `length` apart.
    Rod { length: f32 },
    /// Lets them be anywhere closer than `length`.
    Rope { length: f32 },
    /// A spring that pulls them toward `rest` apart, optionally never closer than `min` nor farther than `max`.
    Spring { rest: f32, hertz: f32, damping: f32, limits: Option<(f32, f32)> },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum JointKind {
    /// Holds two parts as they are when it is made (`WeldConstraint`): it has no frames.
    Weld,
    /// Holds two frames together (`Weld`, `Motor6D`): the parts are put where the frames say.
    FrameWeld,
    /// Stops two parts from colliding with each other (`NoCollisionConstraint`).
    NoCollide,
    Hinge { limit: Option<(f32, f32)>, motor: Option<(f32, f32)> },
    BallSocket { cone: Option<f32>, twist: Option<(f32, f32)> },
    Prismatic { limit: Option<(f32, f32)>, motor: Option<(f32, f32)> },
    Distance(DistanceMode),
}

/// What a constraint instance asks for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct JointSpec {
    pub kind: JointKind,
    pub a: Id,
    /// `None` joins `a` to the world.
    pub b: Option<Id>,
    pub frame_a: (Vec3, Quat),
    pub frame_b: (Vec3, Quat),
    pub collide_connected: bool,
}

struct JointRec {
    _joint: Box<dyn Any>,
    spec: JointSpec,
}

#[derive(Default)]
pub(crate) struct Joints {
    recs: FastMap<Id, JointRec>,
    /// Which constraints refer to a part or an attachment (whether or not they are active), so they are looked at again
    /// when it appears, moves or goes.
    users: FastMap<Id, Vec<Id>>,
    recheck: Vec<Id>,
    /// What a constraint without a `Part1` is joined to: an immovable body at the origin.
    ground: Option<Body<'static>>,
}

impl Joints {
    pub(crate) fn take_rechecks(&mut self) -> Vec<Id> {
        std::mem::take(&mut self.recheck)
    }

    /// A part's body was destroyed: every joint attached to it went with it. (The constraints are looked at again:
    /// a part that comes back, or is replaced, gets them back.)
    pub(crate) fn part_gone(&mut self, part: Id) {
        if let Some(users) = self.users.get(&part).cloned() {
            for joint in users {
                self.recs.remove(&joint);
                self.recheck.push(joint);
            }
        }
    }

    /// A part got a body: the constraints that were waiting for it can be made.
    pub(crate) fn part_appeared(&mut self, part: Id) {
        if let Some(users) = self.users.get(&part) {
            self.recheck.extend(users.iter().copied());
        }
    }

    /// The constraints that refer to this part or attachment.
    pub(crate) fn users_of(&self, id: Id) -> Vec<Id> {
        self.users.get(&id).cloned().unwrap_or_default()
    }

    pub(crate) fn count(&self) -> usize {
        self.recs.len()
    }

    pub(crate) fn clear(&mut self) {
        self.recs.clear();
        self.users.clear();
        self.recheck.clear();
        self.ground = None;
    }

    fn set_users(&mut self, joint: Id, parts: &[Id]) {
        for users in self.users.values_mut() {
            users.retain(|j| *j != joint);
        }
        self.users.retain(|_, u| !u.is_empty());
        for p in parts {
            self.users.entry(*p).or_default().push(joint);
        }
    }
}

fn degrees(d: f32) -> f32 {
    d.to_radians()
}

/// A limit pair in radians, ordered and within what Box3D takes.
fn angle_range(lower_deg: f32, upper_deg: f32) -> (f32, f32) {
    let (lo, hi) = (degrees(lower_deg).clamp(-MAX_ANGLE, MAX_ANGLE), degrees(upper_deg).clamp(-MAX_ANGLE, MAX_ANGLE));
    (lo.min(hi), lo.max(hi))
}

/// What the constraint `id` asks for, or `None` if it is not active.
pub fn read_joint(dm: &DataModel, id: Id) -> Option<JointSpec> {
    let class = dm.get(id)?.class;
    if !class.has_component("Joint") || !dm.flag(id, "Enabled") || !dm.in_workspace(id) {
        return None;
    }
    // A part that was set and has been destroyed makes the constraint inactive (it must not quietly become a joint
    // to the world); a part that was never set is the world.
    let set = |name: &str| match dm.get_prop(id, name) {
        Some(crate::core::Value::Ref(Some(p))) => Some(p),
        _ => None,
    };
    let (raw_a, raw_b) = (set("Part0"), set("Part1"));
    if raw_a.is_some_and(|p| !dm.exists(p)) || raw_b.is_some_and(|p| !dm.exists(p)) {
        return None;
    }
    let cf = |name: &str| dm.get_prop(id, name).and_then(|v| if let crate::core::Value::CF(p, q) = v { Some((p, q)) } else { None }).unwrap_or((Vec3::ZERO, Quat::IDENTITY));
    // Attachments (Roblox' way of joining): each says its part and its frame. One that was set and has been destroyed
    // makes the constraint inactive, like a part that is gone.
    let (side0, side1) = (attachment_side(dm, id, "Attachment0"), attachment_side(dm, id, "Attachment1"));
    if matches!(side0, Side::Gone) || matches!(side1, Side::Gone) {
        return None;
    }
    let by_attachments = !matches!(side0, Side::Unset) || !matches!(side1, Side::Unset);
    let (mut a, mut b) = (raw_a, raw_b);
    let (mut frame_a, mut frame_b) = (cf("C0"), cf("C1"));
    if by_attachments {
        let first = match side0 {
            Side::At(part, frame) => (part, frame),
            _ => (raw_a, cf("C0")),
        };
        let second = match side1 {
            Side::At(part, frame) => (part, frame),
            _ => (raw_b, cf("C1")),
        };
        // (The first body must be a part: when only the second side is on one, the sides trade places.)
        let (first, second) = if first.0.is_none() { (second, first) } else { (first, second) };
        (a, b, frame_a, frame_b) = (first.0, second.0, first.1, second.1);
    } else if a.is_none() {
        a = b.take();
    }
    let a = a?;
    if b == Some(a) {
        return None;
    }
    let num = |name: &str, default: f32| dm.num(id, name, default);
    let limit = |on: &str, lo: &str, hi: &str, dlo: f32, dhi: f32, deg: bool| {
        dm.flag(id, on).then(|| {
            let (lo, hi) = (num(lo, dlo), num(hi, dhi));
            if deg {
                angle_range(lo, hi)
            } else {
                (lo.min(hi), lo.max(hi))
            }
        })
    };
    let motor = |on: &str, speed: &str, max: &str| dm.flag(id, on).then(|| (num(speed, 0.0), num(max, 10_000.0).max(0.0)));
    let kind = match class.name {
        "WeldConstraint" => JointKind::Weld,
        "Weld" | "ManualWeld" | "Motor6D" => JointKind::FrameWeld,
        "NoCollisionConstraint" => JointKind::NoCollide,
        "HingeConstraint" => JointKind::Hinge { limit: limit("LimitsEnabled", "LowerAngle", "UpperAngle", -45.0, 45.0, true), motor: motor("MotorEnabled", "MotorSpeed", "MotorMaxTorque") },
        "BallSocketConstraint" => JointKind::BallSocket {
            cone: dm.flag(id, "LimitsEnabled").then(|| degrees(num("UpperAngle", 45.0)).clamp(0.0, std::f32::consts::PI)),
            twist: limit("TwistLimitsEnabled", "TwistLowerAngle", "TwistUpperAngle", -45.0, 45.0, true),
        },
        "PrismaticConstraint" => JointKind::Prismatic { limit: limit("LimitsEnabled", "LowerLimit", "UpperLimit", 0.0, 10.0, false), motor: motor("MotorEnabled", "MotorSpeed", "MotorMaxForce") },
        "RodConstraint" => JointKind::Distance(DistanceMode::Rod { length: num("Length", 5.0).max(MIN_LENGTH) }),
        "RopeConstraint" => JointKind::Distance(DistanceMode::Rope { length: num("Length", 10.0).max(MIN_LENGTH) }),
        "SpringConstraint" => {
            let limits = dm.flag(id, "LimitsEnabled").then(|| {
                let (lo, hi) = (num("MinLength", 0.0).max(MIN_LENGTH), num("MaxLength", 10.0).max(MIN_LENGTH));
                (lo.min(hi), lo.max(hi))
            });
            let (mut hertz, mut damping) = (num("Frequency", 3.0).max(0.0), num("DampingRatio", 0.5).max(0.0));
            let stiffness = num("Stiffness", 0.0);
            if stiffness > 0.0 {
                // k = m (2 pi f)^2 and c = 2 zeta sqrt(k m), with m the mass the spring moves.
                let mass = match b {
                    Some(b) => crate::physics::analytic_mass(dm, a).min(crate::physics::analytic_mass(dm, b)),
                    None => crate::physics::analytic_mass(dm, a),
                }
                .max(1e-3);
                hertz = (stiffness / mass).sqrt() / std::f32::consts::TAU;
                damping = num("Damping", 0.0) / (2.0 * (stiffness * mass).sqrt());
            }
            JointKind::Distance(DistanceMode::Spring { rest: num("FreeLength", 5.0).max(MIN_LENGTH), hertz, damping, limits })
        }
        _ => return None,
    };
    let free = (Vec3::ZERO, Quat::IDENTITY);
    if matches!(kind, JointKind::Weld | JointKind::NoCollide) {
        (frame_a, frame_b) = (free, free);
    }
    // A Motor6D is a weld whose first frame is moved by its `Transform` (what an animation sets).
    if class.name == "Motor6D" {
        let (tp, tq) = cf("Transform");
        frame_a = (frame_a.0 + frame_a.1 * tp, (frame_a.1 * tq).normalize());
    }
    Some(JointSpec { kind, a, b, frame_a, frame_b, collide_connected: dm.flag(id, "CollideConnected") })
}

/// How far apart the two frames of a constraint are in the world right now, in studs (a constraint that is not active has no distance).
pub fn frames_distance(dm: &DataModel, id: Id) -> Option<f32> {
    let spec = read_joint(dm, id)?;
    let place = |part: Option<Id>, frame: (Vec3, Quat)| match part {
        Some(p) => dm.world_cframe(p).map(|(position, rotation)| position + rotation * frame.0),
        None => Some(frame.0),
    };
    Some((place(Some(spec.a), spec.frame_a)? - place(spec.b, spec.frame_b)?).length())
}

/// One side of a constraint joined by an attachment.
enum Side {
    /// No attachment is set on this side.
    Unset,
    /// The attachment was set and has been destroyed.
    Gone,
    /// The part the attachment is on (none: it is in the world) and its frame (in the part, or in the world).
    At(Option<Id>, (Vec3, Quat)),
}

fn attachment_side(dm: &DataModel, constraint: Id, prop: &str) -> Side {
    let attachment = match dm.get_prop(constraint, prop) {
        Some(crate::core::Value::Ref(Some(a))) => a,
        _ => return Side::Unset,
    };
    if !dm.exists(attachment) {
        return Side::Gone;
    }
    match dm.parent(attachment).filter(|p| dm.is_a(*p, "BasePart")) {
        Some(part) => Side::At(Some(part), dm.cframe(attachment).unwrap_or((Vec3::ZERO, Quat::IDENTITY))),
        None => Side::At(None, dm.world_cframe(attachment).unwrap_or((Vec3::ZERO, Quat::IDENTITY))),
    }
}

/// Turns a frame so that Box3D's Z axis is the frame's X axis.
fn axis_x_as_z(rotation: Quat) -> Quat {
    rotation * Quat::from_rotation_arc(Vec3::Z, Vec3::X)
}

impl PhysicsBridge {
    pub(crate) fn reconcile_joint(&mut self, dm: &mut DataModel, world: &World, id: Id) {
        let wanted = read_joint(dm, id);
        let parts: Vec<Id> = wanted.iter().flat_map(|s| std::iter::once(s.a).chain(s.b)).collect();
        // (A constraint that is not active still has its parts: they are the ones whose arrival activates it.)
        let mut referenced: Vec<Id> = if wanted.is_some() { parts.clone() } else { ["Part0", "Part1"].iter().filter_map(|p| dm.reference(id, p)).collect() };
        // (The attachments, and the parts they are on: moving or destroying an attachment changes the constraint.)
        for prop in ["Attachment0", "Attachment1"] {
            if let Some(attachment) = dm.reference(id, prop) {
                referenced.push(attachment);
                referenced.extend(dm.parent(attachment).filter(|p| dm.is_a(*p, "BasePart")));
            }
        }
        self.joints.set_users(id, &referenced);

        let ready = wanted.filter(|s| self.parts.contains_key(&s.a) && s.b.is_none_or(|b| self.parts.contains_key(&b)));
        match (self.joints.recs.get(&id), ready) {
            // (A joint dies with either of its bodies and `part_gone` drops its record then, so a record that
            // is still here is a joint that is still in the world.)
            (Some(rec), Some(spec)) if rec.spec == spec => {}
            (_, ready) => {
                self.joints.recs.remove(&id);
                if let Some(spec) = ready {
                    match self.build_joint(world, &spec) {
                        Ok(joint) => {
                            self.joints.recs.insert(id, JointRec { _joint: joint, spec });
                        }
                        Err(e) => error!("[physics] could not make the constraint {}: {e}", dm.full_name(id)),
                    }
                }
            }
        }
    }

    fn build_joint(&mut self, world: &World, spec: &JointSpec) -> Result<Box<dyn Any>, String> {
        let a_rec = self.parts.get(&spec.a).ok_or("the first part is not simulated")?;
        let a_pose = a_rec.body.transform();
        let (a_p, a_q) = (Vec3::new(a_pose.p.x, a_pose.p.y, a_pose.p.z), super::bridge::fq(a_pose.q));
        // Frames: `C0` in the first part, `C1` in the second - or, with no second part, in the world, where
        // the immovable body is.
        let (frame_a, frame_b) = match spec.kind {
            JointKind::Weld => {
                // Hold the parts as they are: the first frame is the first part's origin, the second is that
                // same place seen from the second part.
                let b_pose = match spec.b {
                    Some(b) => self.parts.get(&b).ok_or("the second part is not simulated")?.body.transform(),
                    None => box3d::Transform::IDENTITY,
                };
                let (b_p, b_q) = (Vec3::new(b_pose.p.x, b_pose.p.y, b_pose.p.z), super::bridge::fq(b_pose.q));
                let inv = b_q.inverse();
                ((Vec3::ZERO, Quat::IDENTITY), (inv * (a_p - b_p), inv * a_q))
            }
            _ => (spec.frame_a, spec.frame_b),
        };
        let needs_z = matches!(spec.kind, JointKind::Hinge { .. } | JointKind::BallSocket { .. });
        let turn = |f: (Vec3, Quat)| if needs_z { (f.0, axis_x_as_z(f.1)) } else { f };
        let (frame_a, frame_b) = (turn(frame_a), turn(frame_b));

        if spec.b.is_none() && self.joints.ground.is_none() {
            let ground = world.try_create_body(box3d::BodyDef::static_at(box3d::Vec3::ZERO)).map_err(box3d_error)?;
            // SAFETY: as for every handle the bridge keeps (see the module docs of `bridge`).
            self.joints.ground = Some(unsafe { std::mem::transmute::<Body<'_>, Body<'static>>(ground) });
        }
        let a_body = &self.parts.get(&spec.a).ok_or("the first part is not simulated")?.body;
        let b_body: &Body<'static> = match spec.b {
            Some(b) => &self.parts.get(&b).ok_or("the second part is not simulated")?.body,
            None => self.joints.ground.as_ref().ok_or("no world body")?,
        };
        let place = |base: &mut box3d::JointDef<'_, '_>| {
            base.local_frame_a = btransform(frame_a.0, frame_a.1);
            base.local_frame_b = btransform(frame_b.0, frame_b.1);
            base.collide_connected = spec.collide_connected;
        };
        // SAFETY (for each `transmute` below): a joint handle destroys its joint when dropped and checks first
        // that it still exists; the bridge only calls into Box3D while the world is alive.
        let joint: Box<dyn Any> = match spec.kind {
            JointKind::Weld | JointKind::FrameWeld => {
                let mut def = WeldJointDef::new(a_body, b_body);
                place(&mut def.base);
                let j = world.try_create_weld_joint(def).map_err(box3d_error)?;
                Box::new(unsafe { std::mem::transmute::<box3d::WeldJoint<'_>, box3d::WeldJoint<'static>>(j) })
            }
            JointKind::NoCollide => {
                let mut def = box3d::FilterJointDef::new(a_body, b_body);
                place(&mut def.base);
                let j = world.try_create_filter_joint(def).map_err(box3d_error)?;
                Box::new(unsafe { std::mem::transmute::<box3d::FilterJoint<'_>, box3d::FilterJoint<'static>>(j) })
            }
            JointKind::Hinge { limit, motor } => {
                let mut def = RevoluteJointDef::new(a_body, b_body);
                place(&mut def.base);
                if let Some((lo, hi)) = limit {
                    (def.enable_limit, def.lower_angle, def.upper_angle) = (true, lo, hi);
                }
                if let Some((speed, max)) = motor {
                    (def.enable_motor, def.motor_speed, def.max_motor_torque) = (true, speed, max);
                }
                let j = world.try_create_revolute_joint(def).map_err(box3d_error)?;
                Box::new(unsafe { std::mem::transmute::<box3d::RevoluteJoint<'_>, box3d::RevoluteJoint<'static>>(j) })
            }
            JointKind::BallSocket { cone, twist } => {
                let mut def = SphericalJointDef::new(a_body, b_body);
                place(&mut def.base);
                if let Some(angle) = cone {
                    (def.enable_cone_limit, def.cone_angle) = (true, angle);
                }
                if let Some((lo, hi)) = twist {
                    (def.enable_twist_limit, def.lower_twist_angle, def.upper_twist_angle) = (true, lo, hi);
                }
                let j = world.try_create_spherical_joint(def).map_err(box3d_error)?;
                Box::new(unsafe { std::mem::transmute::<box3d::SphericalJoint<'_>, box3d::SphericalJoint<'static>>(j) })
            }
            JointKind::Prismatic { limit, motor } => {
                let mut def = PrismaticJointDef::new(a_body, b_body);
                place(&mut def.base);
                if let Some((lo, hi)) = limit {
                    (def.enable_limit, def.lower_translation, def.upper_translation) = (true, lo, hi);
                }
                if let Some((speed, max)) = motor {
                    (def.enable_motor, def.motor_speed, def.max_motor_force) = (true, speed, max);
                }
                let j = world.try_create_prismatic_joint(def).map_err(box3d_error)?;
                Box::new(unsafe { std::mem::transmute::<box3d::PrismaticJoint<'_>, box3d::PrismaticJoint<'static>>(j) })
            }
            JointKind::Distance(mode) => {
                let mut def = DistanceJointDef::new(a_body, b_body);
                place(&mut def.base);
                match mode {
                    DistanceMode::Rod { length } => def.length = length,
                    DistanceMode::Rope { length } => {
                        // A spring with no stiffness never pulls; the limit is what holds the rope.
                        (def.length, def.enable_spring, def.hertz, def.damping_ratio) = (length, true, 0.0, 0.0);
                        (def.enable_limit, def.min_length, def.max_length) = (true, MIN_LENGTH, length);
                    }
                    DistanceMode::Spring { rest, hertz, damping, limits } => {
                        (def.length, def.enable_spring, def.hertz, def.damping_ratio) = (rest, true, hertz, damping);
                        if let Some((lo, hi)) = limits {
                            (def.enable_limit, def.min_length, def.max_length) = (true, lo, hi);
                        }
                    }
                }
                let j = world.try_create_distance_joint(def).map_err(box3d_error)?;
                Box::new(unsafe { std::mem::transmute::<box3d::DistanceJoint<'_>, box3d::DistanceJoint<'static>>(j) })
            }
        };
        Ok(joint)
    }

    /// Whether the constraint `id` has a joint in the world right now.
    pub fn joint_active(&self, id: Id) -> bool {
        self.joints.recs.contains_key(&id)
    }
}
