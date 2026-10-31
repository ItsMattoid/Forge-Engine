//! Picking: ray casts against the *data model's* collision volumes (exact oriented boxes, ellipsoids and wedges).
//! The editor uses it to select and to place things while nothing is simulated; a running game's `workspace:Raycast`
//! goes through the simulation instead (see [`super::query`]).

use std::collections::HashSet;

use bevy::math::Vec3;

use super::{collider::Plane, ColliderInfo, ColliderShape};
use crate::dm::{DataModel, Id};

#[derive(Default)]
pub struct RayParams {
    pub filter: HashSet<Id>,
    pub include: bool,
}

pub struct Hit {
    pub id: Id,
    pub pos: Vec3,
    pub normal: Vec3,
    pub dist: f32,
}

/// Entry distance of a ray into a box centred on the origin; `None` if it misses or starts inside.
fn ray_box(o: Vec3, d: Vec3, h: Vec3) -> Option<f32> {
    let (mut t_min, mut t_max) = (f32::NEG_INFINITY, f32::INFINITY);
    for i in 0..3 {
        if d[i].abs() < 1e-9 {
            if o[i].abs() > h[i] {
                return None;
            }
        } else {
            let (a, b) = ((-h[i] - o[i]) / d[i], (h[i] - o[i]) / d[i]);
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            t_min = t_min.max(lo);
            t_max = t_max.min(hi);
            if t_min > t_max {
                return None;
            }
        }
    }
    (t_min >= 0.0).then_some(t_min)
}

fn ray_sphere(o: Vec3, d: Vec3, r: f32) -> Option<f32> {
    let a = d.length_squared();
    if a < 1e-12 {
        return None;
    }
    let b = o.dot(d);
    let c = o.length_squared() - r * r;
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / a;
    (t >= 0.0).then_some(t)
}

/// Entry distance of a ray into the convex shape bounded by `planes`, and the entered face's normal
/// (all in the same space); `None` if it misses or starts inside.
fn ray_convex(o: Vec3, d: Vec3, planes: &[Plane]) -> Option<(f32, Vec3)> {
    let (mut t_in, mut t_out) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut entered = Vec3::ZERO;
    for &(n, k) in planes {
        let along = n.dot(d);
        let outside = n.dot(o) - k;
        if along.abs() < 1e-9 {
            if outside > 0.0 {
                return None;
            }
        } else {
            let t = -outside / along;
            if along < 0.0 {
                if t > t_in {
                    t_in = t;
                    entered = n;
                }
            } else {
                t_out = t_out.min(t);
            }
            if t_in > t_out {
                return None;
            }
        }
    }
    (t_in >= 0.0).then_some((t_in, entered))
}

/// Distance along a (unit) ray at which a collision volume is hit, plus the surface normal.
pub fn ray_collider(info: &ColliderInfo, origin: Vec3, dir: Vec3) -> Option<(f32, Vec3)> {
    let size = info.size.max(Vec3::splat(1e-3));
    let inv = info.rotation.inverse();
    let o = inv * (origin - info.center);
    let d = inv * dir;
    if let Some(planes) = info.shape.planes() {
        // Work in the unit box (the shape scaled by `size`): the distance along the ray is unchanged,
        // and a face normal turns by the inverse of the scale.
        let (t, n) = ray_convex(o / size, d / size, planes)?;
        return Some((t, info.rotation * (n / size).normalize_or_zero()));
    }
    if info.shape == ColliderShape::Ball {
        let t = ray_sphere(o / size, d / size, 0.5)?;
        let local = o + d * t;
        let n = (local / (size * size)).normalize_or_zero();
        Some((t, info.rotation * n))
    } else {
        let h = size * 0.5;
        let t = ray_box(o, d, h)?;
        let rel = (o + d * t) / h;
        let axis = if rel.x.abs() >= rel.y.abs() && rel.x.abs() >= rel.z.abs() {
            0
        } else if rel.y.abs() >= rel.z.abs() {
            1
        } else {
            2
        };
        let mut n = Vec3::ZERO;
        n[axis] = rel[axis].signum();
        Some((t, info.rotation * n))
    }
}

/// `dir` is both direction and maximum distance (like Roblox).
pub fn raycast(dm: &DataModel, origin: Vec3, dir: Vec3, rp: &RayParams) -> Option<Hit> {
    let len = dir.length();
    if len < 1e-6 {
        return None;
    }
    let d = dir / len;
    let mut best: Option<Hit> = None;
    for id in dm.colliders() {
        if !dm.in_workspace(id) {
            continue;
        }
        // Exclude mode skips listed objects; include mode skips everything that is not listed.
        let listed = rp.filter.iter().any(|f| *f == id || dm.is_descendant_of(id, *f));
        if rp.include != listed {
            continue;
        }
        let Some(info) = dm.collider(id) else { continue };
        let Some((t, normal)) = ray_collider(&info, origin, d) else { continue };
        if t > len || best.as_ref().is_some_and(|b| b.dist <= t) {
            continue;
        }
        best = Some(Hit { id, pos: origin + d * t, normal, dist: t });
    }
    best
}

/// Used by the editor: everything under a ray, nearest first (what Alt-click cycles through).
/// Each object appears once, at the distance where the ray first enters it.
pub fn pick_colliders_all(dm: &DataModel, origin: Vec3, dir: Vec3) -> Vec<(Id, f32)> {
    let d = dir.normalize_or_zero();
    if d == Vec3::ZERO {
        return Vec::new();
    }
    let mut hits: Vec<(Id, f32)> = dm
        .colliders()
        .into_iter()
        .filter(|id| dm.in_workspace(*id))
        .filter_map(|id| {
            let info = dm.collider(id)?;
            ray_collider(&info, origin, d).map(|(t, _)| (id, t))
        })
        .collect();
    // Nearest first; equal distances keep the (deterministic) component order.
    hits.sort_by(|a, b| a.1.total_cmp(&b.1));
    hits
}

/// Used by the editor: nearest collidable object under a ray, ignoring filters.
pub fn pick_collider(dm: &DataModel, origin: Vec3, dir: Vec3) -> Option<(Id, f32)> {
    let d = dir.normalize_or_zero();
    let mut best: Option<(Id, f32)> = None;
    for id in dm.colliders() {
        if !dm.in_workspace(id) {
            continue;
        }
        let Some(info) = dm.collider(id) else { continue };
        if let Some((t, _)) = ray_collider(&info, origin, d) {
            if best.is_none_or(|(_, bt)| t < bt) {
                best = Some((id, t));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use bevy::math::Quat;

    use super::*;
    use crate::physics::ColliderShape;

    fn info(shape: ColliderShape) -> ColliderInfo {
        ColliderInfo { center: Vec3::ZERO, rotation: Quat::IDENTITY, size: Vec3::splat(2.0), shape, can_collide: true, can_touch: true, anchored: true }
    }

    fn cast(shape: ColliderShape, origin: Vec3, dir: Vec3) -> Option<(f32, Vec3)> {
        ray_collider(&info(shape), origin, dir)
    }

    #[test]
    fn a_wedge_is_hit_on_its_slope_and_not_where_it_is_cut_away() {
        let slope = Vec3::new(0.0, 1.0, -1.0).normalize();
        // From above, over the tall (back) half: the slope is hit below the top of the box.
        let (t, n) = cast(ColliderShape::Wedge, Vec3::new(0.0, 5.0, 0.8), Vec3::NEG_Y).unwrap();
        assert!((t - 4.2).abs() < 1e-4, "{t}");
        assert!(n.abs_diff_eq(slope, 1e-4), "{n}");
        // Straight at the front: the box would be hit at once (t = 4), the wedge only where the slope is.
        let (t, n) = cast(ColliderShape::Wedge, Vec3::new(0.0, 0.5, -5.0), Vec3::Z).unwrap();
        assert!((t - 5.5).abs() < 1e-4, "{t}");
        assert!(n.abs_diff_eq(slope, 1e-4));
        // From behind it is a flat wall, and from below a flat floor.
        assert!(cast(ColliderShape::Wedge, Vec3::new(0.0, 0.5, 5.0), Vec3::NEG_Z).is_some_and(|(t, n)| (t - 4.0).abs() < 1e-4 && n.abs_diff_eq(Vec3::Z, 1e-4)));
        assert!(cast(ColliderShape::Wedge, Vec3::new(0.0, -5.0, -0.5), Vec3::Y).is_some_and(|(t, n)| (t - 4.0).abs() < 1e-4 && n.abs_diff_eq(Vec3::NEG_Y, 1e-4)));
        // Above the front edge there is nothing.
        assert!(cast(ColliderShape::Wedge, Vec3::new(0.0, 5.0, -0.9), Vec3::Z).is_none());
        // The size scales the shape and the normals follow (a long, shallow slope).
        let mut flat = info(ColliderShape::Wedge);
        flat.size = Vec3::new(2.0, 1.0, 4.0);
        let (_, n) = ray_collider(&flat, Vec3::new(0.0, 5.0, 1.0), Vec3::NEG_Y).unwrap();
        assert!(n.abs_diff_eq(Vec3::new(0.0, 1.0, -0.25).normalize(), 1e-4), "{n}");
    }

    #[test]
    fn a_corner_wedge_has_a_tip_above_one_corner() {
        let slope = Vec3::new(0.0, 1.0, -1.0).normalize();
        // Over the tip's corner the part is as tall as the box.
        let (t, _) = cast(ColliderShape::CornerWedge, Vec3::new(0.99, 5.0, 0.99), Vec3::NEG_Y).unwrap();
        assert!((t - 4.01).abs() < 1e-3, "{t}");
        let (t, n) = cast(ColliderShape::CornerWedge, Vec3::new(0.9, 5.0, 0.5), Vec3::NEG_Y).unwrap();
        assert!((t - 4.5).abs() < 1e-4 && n.abs_diff_eq(slope, 1e-4), "{t} {n}");
        // Over the opposite corner it is only a sliver above the floor.
        let (t, _) = cast(ColliderShape::CornerWedge, Vec3::new(-0.8, 5.0, 0.8), Vec3::NEG_Y).unwrap();
        assert!((t - 5.8).abs() < 1e-4, "{t}");
        // A ray from the side that cuts the far corner misses it entirely.
        assert!(cast(ColliderShape::CornerWedge, Vec3::new(-5.0, 0.5, -0.9), Vec3::X).is_none());
    }

    #[test]
    fn the_plain_shapes_still_behave() {
        assert!(cast(ColliderShape::Box, Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y).is_some_and(|(t, _)| (t - 4.0).abs() < 1e-4));
        assert!(cast(ColliderShape::Ball, Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y).is_some_and(|(t, _)| (t - 4.0).abs() < 1e-4));
        assert!(cast(ColliderShape::Ball, Vec3::new(0.95, 5.0, 0.95), Vec3::NEG_Y).is_none(), "a ball's corner is empty");
    }
}
