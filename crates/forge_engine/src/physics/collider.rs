//! Collision volumes as the *data model* describes them: what a component's `collider` function produces. The
//! editor picks and draws with these while nothing is simulated; the simulation builds its own shapes from the
//! same description (see [`super::shapes`]).

use bevy::math::{Mat3, Quat, Vec3};

use crate::dm::{DataModel, Id};

/// The axis-aligned box around an oriented box (p its centre, q its rotation, size its sides).
pub fn world_aabb(p: Vec3, q: Quat, size: Vec3) -> (Vec3, Vec3) {
    let m = Mat3::from_quat(q);
    let h = size * 0.5;
    let e = m.x_axis.abs() * h.x + m.y_axis.abs() * h.y + m.z_axis.abs() * h.z;
    (p - e, p + e)
}

/// The shape of a collision volume inside its box (the box is `size` wide, centred on `center`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColliderShape {
    Box,
    /// An ellipsoid inscribed in the box.
    Ball,
    /// The box cut in half along a slope: the low edge is at the front (-Z), the tall wall at the back (+Z).
    Wedge,
    /// A pyramid on the box's floor whose tip is above the back-right bottom corner.
    CornerWedge,
}

/// A face of a convex shape in the unit box (-0.5..0.5 on every axis): the points `p` with `normal . p <= offset`.
pub type Plane = (Vec3, f32);

const WEDGE: [Plane; 5] = [(Vec3::new(0.0, -1.0, 0.0), 0.5), (Vec3::new(0.0, 0.0, 1.0), 0.5), (Vec3::new(1.0, 0.0, 0.0), 0.5), (Vec3::new(-1.0, 0.0, 0.0), 0.5), (Vec3::new(0.0, 1.0, -1.0), 0.0)];

const CORNER_WEDGE: [Plane; 5] =
    [(Vec3::new(0.0, -1.0, 0.0), 0.5), (Vec3::new(0.0, 0.0, 1.0), 0.5), (Vec3::new(1.0, 0.0, 0.0), 0.5), (Vec3::new(0.0, 1.0, -1.0), 0.0), (Vec3::new(-1.0, 1.0, 0.0), 0.0)];

impl ColliderShape {
    /// The faces of a flat-sided shape other than the plain box (its volume is where all of them hold).
    pub fn planes(self) -> Option<&'static [Plane]> {
        match self {
            ColliderShape::Wedge => Some(&WEDGE),
            ColliderShape::CornerWedge => Some(&CORNER_WEDGE),
            ColliderShape::Box | ColliderShape::Ball => None,
        }
    }
}

/// An instance's collision volume (produced by a component's `collider` function).
#[derive(Clone, Copy, Debug)]
pub struct ColliderInfo {
    pub center: Vec3,
    pub rotation: Quat,
    pub size: Vec3,
    /// What fills the box.
    pub shape: ColliderShape,
    pub can_collide: bool,
    pub can_touch: bool,
    /// Fixed in place.
    pub anchored: bool,
}

impl ColliderInfo {
    /// The axis-aligned box around the volume's whole box.
    pub fn aabb(&self) -> (Vec3, Vec3) {
        world_aabb(self.center, self.rotation, self.size)
    }
}

/// Describes an instance's collision volume, or `None` if it has none right now.
pub type ColliderFn = fn(&DataModel, Id) -> Option<ColliderInfo>;
