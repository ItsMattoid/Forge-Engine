//! The solid shape of a part, in the form Box3D takes it, and the arithmetic that goes with it (volume, mass,
//! the corner points a query uses).
//!
//! A part's [`ColliderShape`] and `Size` give one convex solid centred on the part's position:
//!
//! * a block is a box;
//! * a ball whose three sizes are equal is a sphere; a stretched one (an ellipsoid) has no Box3D primitive, so it
//!   is the convex hull of 42 points on the ellipsoid (a 10-sided, 5-ring cage: its surface never lies outside the
//!   ellipsoid and its volume is a little under it - Box3D hulls are limited to 128 edges);
//! * the wedge and the corner wedge are exact convex hulls of their corners.
//!
//! Sizes the simulation cannot hold are changed, never rejected: a part thinner than [`MIN_SIZE`] collides as
//! if it were that thin, and one with a coordinate outside [`world_limit`] is not simulated at all (Box3D's
//! world is finite).

use bevy::math::{Quat, Vec3};

use super::ColliderShape;

/// The thinnest a part collides: four times Box3D's linear slop (0.005 m, in studs). Thinner than that and the solver
/// cannot tell the two faces apart.
pub fn min_size() -> f32 {
    4.0 * 0.005 * box3d::length_units_per_meter()
}

/// How far from the origin a part (its bounding box) may reach and still be simulated.
pub fn world_limit() -> f32 {
    0.45 * 1.0e5 * box3d::length_units_per_meter()
}

/// The size a part collides with: each side held between [`min_size`] and what the world can hold.
pub fn collision_size(size: Vec3) -> Vec3 {
    let hi = world_limit();
    Vec3::new(size.x.clamp(min_size(), hi), size.y.clamp(min_size(), hi), size.z.clamp(min_size(), hi))
}

/// What Box3D is given for a part.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Box {
        half: Vec3,
    },
    Sphere {
        radius: f32,
    },
    /// The corners of a convex solid, centred on the part's origin.
    Hull(Vec<Vec3>),
}

/// How equal three sizes have to be for an ellipsoid to be a sphere.
const SPHERE_TOLERANCE: f32 = 1.001;

pub fn geometry(shape: ColliderShape, size: Vec3) -> Geometry {
    let size = collision_size(size);
    let half = size * 0.5;
    match shape {
        ColliderShape::Box => Geometry::Box { half },
        ColliderShape::Ball => {
            if size.max_element() <= size.min_element() * SPHERE_TOLERANCE {
                Geometry::Sphere { radius: size.min_element() * 0.5 }
            } else {
                Geometry::Hull(ellipsoid_points(half))
            }
        }
        ColliderShape::Wedge => Geometry::Hull(wedge_points(half)),
        ColliderShape::CornerWedge => Geometry::Hull(corner_wedge_points(half)),
    }
}

/// The wedge's six corners: the low edge is at the front (-Z), the tall wall at the back (+Z).
fn wedge_points(h: Vec3) -> Vec<Vec3> {
    let mut points = Vec::with_capacity(6);
    for x in [-h.x, h.x] {
        points.push(Vec3::new(x, -h.y, -h.z));
        points.push(Vec3::new(x, -h.y, h.z));
        points.push(Vec3::new(x, h.y, h.z));
    }
    points
}

/// The corner wedge's five corners: the floor's four and the tip above the back-right corner.
fn corner_wedge_points(h: Vec3) -> Vec<Vec3> {
    vec![Vec3::new(-h.x, -h.y, -h.z), Vec3::new(h.x, -h.y, -h.z), Vec3::new(-h.x, -h.y, h.z), Vec3::new(h.x, -h.y, h.z), Vec3::new(h.x, h.y, h.z)]
}

// (Box3D allows a hull 128 edges: a hull of V points has at most 3V - 6, so 42 points fit.)
const ELLIPSOID_SEGMENTS: usize = 10;
const ELLIPSOID_RINGS: usize = 5;

/// Points on an ellipsoid with these half sizes: the two poles and rings of points between them.
fn ellipsoid_points(h: Vec3) -> Vec<Vec3> {
    let mut points = Vec::with_capacity(2 + ELLIPSOID_SEGMENTS * (ELLIPSOID_RINGS - 1));
    points.push(Vec3::new(0.0, h.y, 0.0));
    points.push(Vec3::new(0.0, -h.y, 0.0));
    for ring in 1..ELLIPSOID_RINGS {
        let polar = std::f32::consts::PI * ring as f32 / ELLIPSOID_RINGS as f32;
        let (sin_p, cos_p) = polar.sin_cos();
        for seg in 0..ELLIPSOID_SEGMENTS {
            let azimuth = std::f32::consts::TAU * seg as f32 / ELLIPSOID_SEGMENTS as f32;
            let (sin_a, cos_a) = azimuth.sin_cos();
            points.push(Vec3::new(h.x * sin_p * cos_a, h.y * cos_p, h.z * sin_p * sin_a));
        }
    }
    points
}

/// The volume of the solid, in cubic studs.
pub fn volume(shape: ColliderShape, size: Vec3) -> f32 {
    let size = collision_size(size);
    let block = size.x * size.y * size.z;
    match shape {
        ColliderShape::Box => block,
        ColliderShape::Ball => block * std::f32::consts::FRAC_PI_6,
        ColliderShape::Wedge => block * 0.5,
        ColliderShape::CornerWedge => block / 3.0,
    }
}

/// The points of the solid in world space relative to its centre, for a query that tests the part's shape
/// (every convex solid is the hull of its corners; a sphere is its centre and a radius).
pub fn query_proxy(shape: ColliderShape, size: Vec3, rotation: Quat) -> (Vec<Vec3>, f32) {
    match geometry(shape, size) {
        Geometry::Box { half } => {
            let mut corners = Vec::with_capacity(8);
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    for sz in [-1.0, 1.0] {
                        corners.push(rotation * (half * Vec3::new(sx, sy, sz)));
                    }
                }
            }
            (corners, 0.0)
        }
        Geometry::Sphere { radius } => (vec![Vec3::ZERO], radius),
        Geometry::Hull(points) => (points.into_iter().map(|p| rotation * p).collect(), 0.0),
    }
}

/// Whether a position and size lie inside the world the simulation can hold.
pub fn in_world(center: Vec3, size: Vec3) -> bool {
    let reach = collision_size(size).max_element() * 0.5 * 1.74; // (half the diagonal of a cube)
    let limit = world_limit();
    center.is_finite() && center.abs().max_element() + reach <= limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_a_box_and_an_even_ball_a_sphere() {
        assert_eq!(geometry(ColliderShape::Box, Vec3::new(4.0, 1.0, 2.0)), Geometry::Box { half: Vec3::new(2.0, 0.5, 1.0) });
        assert_eq!(geometry(ColliderShape::Ball, Vec3::splat(4.0)), Geometry::Sphere { radius: 2.0 });
        assert!(matches!(geometry(ColliderShape::Ball, Vec3::new(4.0, 2.0, 4.0)), Geometry::Hull(p) if p.len() == 2 + 10 * 4));
    }

    #[test]
    fn the_wedges_are_hulls_of_their_corners() {
        let Geometry::Hull(w) = geometry(ColliderShape::Wedge, Vec3::new(2.0, 2.0, 4.0)) else { panic!("not a hull") };
        assert_eq!(w.len(), 6);
        assert!(w.contains(&Vec3::new(1.0, 1.0, 2.0)) && w.contains(&Vec3::new(-1.0, -1.0, -2.0)));
        assert!(!w.contains(&Vec3::new(1.0, 1.0, -2.0)), "the front top edge is cut away");
        let Geometry::Hull(c) = geometry(ColliderShape::CornerWedge, Vec3::splat(2.0)) else { panic!("not a hull") };
        assert_eq!(c.len(), 5);
        assert!(c.contains(&Vec3::new(1.0, 1.0, 1.0)), "the tip is above the back-right corner");
    }

    #[test]
    fn volumes_follow_the_shape() {
        let size = Vec3::new(2.0, 3.0, 4.0);
        assert_eq!(volume(ColliderShape::Box, size), 24.0);
        assert_eq!(volume(ColliderShape::Wedge, size), 12.0);
        assert!((volume(ColliderShape::CornerWedge, size) - 8.0).abs() < 1e-5);
        assert!((volume(ColliderShape::Ball, Vec3::splat(2.0)) - 4.0 / 3.0 * std::f32::consts::PI).abs() < 1e-4);
    }

    #[test]
    fn sizes_the_simulation_cannot_hold_are_changed_not_rejected() {
        let g = geometry(ColliderShape::Box, Vec3::new(0.001, 5.0, 1.0e9));
        let Geometry::Box { half } = g else { panic!("not a box") };
        assert!(half.x >= min_size() * 0.5 && half.z <= world_limit() * 0.5 + 1.0);
        assert!(in_world(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0e9)), "a part that big is simulated as the biggest one the world holds");
        assert!(!in_world(Vec3::new(40_000.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0e9)), "... and where it would reach outside the world it is not simulated");
        assert!(!in_world(Vec3::new(f32::INFINITY, 0.0, 0.0), Vec3::ONE));
        assert!(!in_world(Vec3::new(0.0, 5.0e6, 0.0), Vec3::ONE));
        assert!(in_world(Vec3::new(1000.0, -500.0, 0.0), Vec3::new(500.0, 10.0, 500.0)));
    }

    #[test]
    fn the_query_proxy_turns_with_the_part() {
        let (points, radius) = query_proxy(ColliderShape::Box, Vec3::new(2.0, 2.0, 2.0), Quat::from_rotation_y(std::f32::consts::FRAC_PI_4));
        assert_eq!((points.len(), radius), (8, 0.0));
        let widest = points.iter().map(|p| p.x.abs()).fold(0.0, f32::max);
        assert!((widest - 2.0_f32.sqrt()).abs() < 1e-5, "{widest}");
        assert_eq!(query_proxy(ColliderShape::Ball, Vec3::splat(6.0), Quat::IDENTITY), (vec![Vec3::ZERO], 3.0));
    }
}
