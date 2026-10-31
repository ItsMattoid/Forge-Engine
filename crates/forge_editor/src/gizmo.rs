//! The maths behind the move / scale / rotate handles: where the handles are, which one is under
//! the mouse and what a drag does. All plain functions, so they can be tested without a window.

use bevy::math::{Quat, Vec2, Vec3};

use crate::{settings::Space, state::Tool};

pub const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

/// The three handle directions. Scale always follows the object (a size is along its own edges);
/// move and rotate follow the chosen space.
pub fn axis_dirs(tool: Tool, space: Space, pivot_rotation: Quat) -> [Vec3; 3] {
    let local = tool == Tool::Scale || space == Space::Local;
    AXES.map(|a| if local { (pivot_rotation * a).normalize() } else { a })
}

/// The editor camera and the viewport it draws into, enough to turn a screen position into a ray
/// and a world position into a screen position (egui points, the same units as the pointer).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub pos: Vec3,
    pub rot: Quat,
    /// Vertical field of view in radians.
    pub fov: f32,
    pub min: bevy::math::Vec2,
    pub size: bevy::math::Vec2,
}

impl View {
    /// The ray through a screen position (origin, unit direction).
    pub fn ray(&self, p: bevy::math::Vec2) -> Option<(Vec3, Vec3)> {
        if self.size.x < 2.0 || self.size.y < 2.0 {
            return None;
        }
        let ndc = (p - self.min) / self.size * 2.0 - bevy::math::Vec2::ONE;
        let t = (self.fov * 0.5).tan();
        let local = Vec3::new(ndc.x * t * self.size.x / self.size.y, -ndc.y * t, -1.0);
        Some((self.pos, (self.rot * local).normalize()))
    }

    /// Where a world position appears on screen (`None` behind the camera).
    pub fn project(&self, world: Vec3) -> Option<bevy::math::Vec2> {
        if self.size.x < 2.0 || self.size.y < 2.0 {
            return None;
        }
        let v = self.rot.inverse() * (world - self.pos);
        if v.z > -0.05 {
            return None;
        }
        let t = (self.fov * 0.5).tan();
        let ndc = bevy::math::Vec2::new(v.x / (-v.z * t * self.size.x / self.size.y), -v.y / (-v.z * t));
        Some(self.min + (ndc * 0.5 + bevy::math::Vec2::splat(0.5)) * self.size)
    }

    /// The direction the camera looks in.
    pub fn forward(&self) -> Vec3 {
        self.rot * Vec3::NEG_Z
    }

    /// How many screen points one world unit is across at `world` (facing the camera).
    pub fn pixels_per_unit(&self, world: Vec3) -> f32 {
        let depth = (world - self.pos).dot(self.forward()).max(0.05);
        self.size.y / (2.0 * (self.fov * 0.5).tan() * depth)
    }
}

// ---- move handles: six arrows and three planes -----------------------------------------------------

/// Which handle of the move tool.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MoveHandle {
    /// One of the six arrows: axis (0..3) and direction (+1 / -1).
    Arrow(usize, f32),
    /// A plane handle, named by the axis that is *not* moved (so `Plane(1)` moves X and Z).
    Plane(usize),
}

/// The two axes a plane handle moves, in order.
pub fn plane_axes(normal: usize) -> (usize, usize) {
    match normal {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// The side of each axis that faces the camera (the plane handles sit there so they never hide
/// behind the object or point away from you).
pub fn facing_signs(camera: Vec3, origin: Vec3, dirs: &[Vec3; 3]) -> [f32; 3] {
    let to_camera = camera - origin;
    dirs.map(|d| if d.dot(to_camera) >= 0.0 { 1.0 } else { -1.0 })
}

/// The empty space around the centre of the move gizmo, as a fraction of the handle length: the
/// arrows start this far out so the object (and the other handles) stay visible in the middle.
pub const ARROW_GAP: f32 = 0.22;
/// The solid cone at the end of an arrow: its length and base radius, as fractions of the handle length.
pub const CONE_LEN: f32 = 0.2;
pub const CONE_RADIUS: f32 = 0.07;

/// Where the plane handle sits along each of its axes, as fractions of the handle length. It is drawn
/// small so it stays out of the way; it is picked from a much bigger area (see [`hit_move`]).
pub const PLANE_NEAR: f32 = 0.30;
pub const PLANE_FAR: f32 = 0.46;
/// A plane seen more edge-on than this (the cosine of the angle between the view direction and the
/// plane's normal) cannot be picked, and a drag on it holds still: where the mouse ray meets such a
/// plane is far away and jumps about wildly.
pub const MIN_PLANE_FACING: f32 = 0.08;
/// What a plane drag accepts (a little looser than picking, so a drag does not stall at the limit).
const MIN_PLANE_DRAG_FACING: f32 = 0.05;

/// The four corners of a plane handle (in order around the square).
pub fn plane_corners(origin: Vec3, dirs: &[Vec3; 3], signs: &[f32; 3], normal: usize, len: f32) -> [Vec3; 4] {
    let (a, b) = plane_axes(normal);
    let (da, db) = (dirs[a] * signs[a], dirs[b] * signs[b]);
    let at = |u: f32, v: f32| origin + da * (u * len) + db * (v * len);
    [at(PLANE_NEAR, PLANE_NEAR), at(PLANE_FAR, PLANE_NEAR), at(PLANE_FAR, PLANE_FAR), at(PLANE_NEAR, PLANE_FAR)]
}

/// The distance from `p` to a convex polygon on screen: 0 inside it.
pub fn point_polygon_distance(p: Vec2, poly: &[Vec2]) -> f32 {
    if poly.len() < 3 {
        return poly.iter().map(|q| q.distance(p)).fold(f32::MAX, f32::min);
    }
    let sides: Vec<f32> = (0..poly.len()).map(|i| (poly[(i + 1) % poly.len()] - poly[i]).perp_dot(p - poly[i])).collect();
    if sides.iter().all(|s| *s >= 0.0) || sides.iter().all(|s| *s <= 0.0) {
        return 0.0;
    }
    (0..poly.len()).map(|i| point_segment_distance(p, poly[i], poly[(i + 1) % poly.len()])).fold(f32::MAX, f32::min)
}

/// The distance from `p` to the segment `a..b`.
pub fn point_segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 < 1e-12 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}

/// How far (screen points) from a plane handle's drawn square the mouse still picks it. It follows the
/// size of the gizmo on screen, so a small viewport or a far-away object keeps a usable area.
pub fn plane_pick_pad(len_on_screen: f32) -> f32 {
    (len_on_screen * 0.16).clamp(6.0, 18.0)
}

/// How close (screen points) the mouse has to be to an arrow to pick it: it follows the size of the gizmo on screen.
pub fn arrow_pick_tolerance(len_on_screen: f32) -> f32 {
    (len_on_screen * 0.11).clamp(6.0, 12.0)
}

/// Directly on an arrow's line (this close, screen points) the arrow is picked even over a plane's square.
pub const ARROW_CORE: f32 = 4.0;

/// The arrow nearest the mouse on screen, and how far it is (screen points); on a tie the nearer to the camera.
fn nearest_arrow(view: &View, pointer: Vec2, origin: Vec3, dirs: &[Vec3; 3], len: f32) -> Option<(MoveHandle, f32)> {
    let mut best: Option<(MoveHandle, f32, f32)> = None;
    for (i, axis) in dirs.iter().enumerate() {
        for sign in [1.0f32, -1.0] {
            let (from, to) = (origin + *axis * (sign * len * ARROW_GAP), origin + *axis * (sign * len));
            let (Some(a), Some(b)) = (view.project(from), view.project(to)) else { continue };
            let distance = point_segment_distance(pointer, a, b);
            let depth = ((from + to) * 0.5 - view.pos).length();
            if best.is_none_or(|(_, d, z)| distance < d - 0.5 || (distance <= d + 0.5 && depth < z)) {
                best = Some((MoveHandle::Arrow(i, sign), distance, depth));
            }
        }
    }
    best.map(|(h, d, _)| (h, d))
}

/// The plane handle nearest the mouse on screen, and how far its drawn square is from it (0: inside it),
/// if that is within the pick area. The handles are drawn small, but the area that picks them is the drawn
/// square grown by [`plane_pick_pad`] on every side, measured on screen, so it stays easy to grab at any
/// distance and angle (a plane seen at a slant is a thin sliver, which its padding makes wide enough).
/// When two overlap the closer one on screen wins, then the nearer to the camera.
fn nearest_plane(view: &View, pointer: Vec2, origin: Vec3, dirs: &[Vec3; 3], len: f32) -> Option<(usize, f32)> {
    let to_origin = (origin - view.pos).normalize_or_zero();
    let signs = facing_signs(view.pos, origin, dirs);
    let pad = plane_pick_pad(len * view.pixels_per_unit(origin));
    let mut best: Option<(usize, f32, f32)> = None;
    for normal in 0..3 {
        if to_origin.dot(dirs[normal]).abs() < MIN_PLANE_FACING {
            continue;
        }
        let corners = plane_corners(origin, dirs, &signs, normal, len);
        let Some(screen) = corners.iter().map(|c| view.project(*c)).collect::<Option<Vec<Vec2>>>() else { continue };
        let distance = point_polygon_distance(pointer, &screen);
        let depth = (corners[0] + corners[2] - view.pos * 2.0).length() * 0.5;
        if distance <= pad && best.is_none_or(|(_, d, z)| distance < d - 0.5 || (distance <= d + 0.5 && depth < z)) {
            best = Some((normal, distance, depth));
        }
    }
    best.map(|(n, d, _)| (n, d))
}

/// The move handle under the mouse. Everything is judged on screen, where the mouse is:
/// * an arrow is picked within a few points of its line (see [`arrow_pick_tolerance`]);
/// * a plane is picked from its drawn square and a margin round it (see [`plane_pick_pad`]);
/// * where they overlap the arrow wins, since it is thin and the plane's margin would otherwise swallow it,
///   except inside the drawn square itself and not right on the arrow's line ([`ARROW_CORE`]): seen at a slant
///   a square can lie right beside an arrow, and being inside the square is a plain request for the plane.
pub fn hit_move(view: &View, pointer: Vec2, origin: Vec3, dirs: &[Vec3; 3]) -> Option<MoveHandle> {
    let len = handle_len(view.pos, origin);
    let tolerance = arrow_pick_tolerance(len * view.pixels_per_unit(origin));
    let arrow = nearest_arrow(view, pointer, origin, dirs, len).filter(|(_, d)| *d <= tolerance);
    let plane = nearest_plane(view, pointer, origin, dirs, len);
    match (arrow, plane) {
        (Some((_, on_arrow)), Some((normal, in_square))) if in_square == 0.0 && on_arrow > ARROW_CORE => Some(MoveHandle::Plane(normal)),
        (Some((handle, _)), _) => Some(handle),
        (None, Some((normal, _))) => Some(MoveHandle::Plane(normal)),
        (None, None) => None,
    }
}
/// The base circle of a cone (`count` points) with its tip last: the points a flat cone silhouette is made from.
pub fn cone_points(base_center: Vec3, tip: Vec3, radius: f32, count: usize) -> Vec<Vec3> {
    let axis = (tip - base_center).normalize_or_zero();
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    let mut points: Vec<Vec3> = (0..count)
        .map(|i| {
            let a = i as f32 / count as f32 * std::f32::consts::TAU;
            base_center + (u * a.cos() + v * a.sin()) * radius
        })
        .collect();
    points.push(tip);
    points
}

/// The convex hull of 2D points, counter-clockwise (the outline of a flat-shaded cone on screen).
pub fn convex_hull(points: &[bevy::math::Vec2]) -> Vec<bevy::math::Vec2> {
    use bevy::math::Vec2;
    let mut pts: Vec<Vec2> = points.to_vec();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<Vec2> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let ordered: Vec<Vec2> = if pass == 0 { pts.clone() } else { pts.iter().rev().copied().collect() };
        for p in ordered {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// The union of several world-aligned boxes (given as min / max corners): centre and half size.
pub fn union_box(boxes: impl IntoIterator<Item = (Vec3, Vec3)>) -> Option<(Vec3, Vec3)> {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut any = false;
    for (a, b) in boxes {
        min = min.min(a);
        max = max.max(b);
        any = true;
    }
    any.then(|| ((min + max) * 0.5, (max - min) * 0.5))
}

// ---- scale handles: six spheres on the sides of the bounding box -------------------------------------

/// An oriented box: where the scale handles go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxBounds {
    pub center: Vec3,
    /// Half the size along each of `rot`'s axes.
    pub half: Vec3,
    pub rot: Quat,
}

impl BoxBounds {
    /// The centre of the face on the `sign` side of axis `axis`.
    pub fn face(&self, axis: usize, sign: f32) -> Vec3 {
        self.center + self.rot * (AXES[axis] * (sign * self.half[axis]))
    }

    /// The outward direction of that face.
    pub fn outward(&self, axis: usize, sign: f32) -> Vec3 {
        (self.rot * AXES[axis] * sign).normalize()
    }

    /// Where the scale sphere for a face is: a little way out from the face, so the handles do not sit
    /// on the object (and, at the corners of a box, do not crowd each other).
    pub fn handle(&self, camera: Vec3, axis: usize, sign: f32) -> Vec3 {
        let face = self.face(axis, sign);
        face + self.outward(axis, sign) * (sphere_radius(camera, face) * 2.6)
    }
}

/// How big the handle spheres are drawn and picked: constant on screen.
pub fn sphere_radius(camera: Vec3, point: Vec3) -> f32 {
    ((point - camera).length() * 0.017).max(0.05)
}

fn ray_sphere(ray: (Vec3, Vec3), center: Vec3, radius: f32) -> Option<f32> {
    let oc = ray.0 - center;
    let b = oc.dot(ray.1);
    let c = oc.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t > 0.0).then_some(t).or_else(|| (-b + disc.sqrt() > 0.0).then_some(0.0))
}

/// The scale sphere (axis, side) under the ray; the nearest wins.
pub fn hit_scale(camera: Vec3, ray: (Vec3, Vec3), bounds: &BoxBounds) -> Option<(usize, f32)> {
    let mut best: Option<((usize, f32), f32)> = None;
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let p = bounds.handle(camera, axis, sign);
            // A little bigger than it is drawn, so it is easy to grab.
            let Some(t) = ray_sphere(ray, p, sphere_radius(camera, p) * 1.6) else { continue };
            if best.is_none_or(|(_, bt)| t < bt) {
                best = Some(((axis, sign), t));
            }
        }
    }
    best.map(|(h, _)| h)
}

// ---- rotation: rings, trackball and tick marks ---------------------------------------------------------

/// A point on the virtual trackball sphere for a ray: where the ray meets the sphere, or the nearest
/// point of its outline when the ray misses (so dragging past the edge keeps turning smoothly).
pub fn trackball_point(ray: (Vec3, Vec3), origin: Vec3, radius: f32) -> Vec3 {
    if let Some(t) = ray_sphere(ray, origin, radius) {
        return ray.0 + ray.1 * t - origin;
    }
    let closest = ray.0 + ray.1 * (origin - ray.0).dot(ray.1);
    let off = closest - origin;
    // Straight at the centre only happens for a hit, but stay safe.
    if off.length_squared() < 1e-12 {
        return -ray.1 * radius;
    }
    off.normalize() * radius
}

/// Whether the mouse ray passes through the trackball region (the disc inside the rings).
pub fn inside_trackball(ray: (Vec3, Vec3), origin: Vec3, radius: f32) -> bool {
    let closest = ray.0 + ray.1 * (origin - ray.0).dot(ray.1);
    (closest - origin).length() < radius && (origin - ray.0).dot(ray.1) > 0.0
}

/// The rotation that carries trackball point `from` onto `to`: the short way, so never more than half a turn.
/// A drag must therefore add up these small steps (frame to frame) and not measure from where it began, or it
/// could never turn the object further than that one arc.
pub fn trackball_rotation(from: Vec3, to: Vec3) -> Quat {
    if from.length_squared() < 1e-12 || to.length_squared() < 1e-12 {
        return Quat::IDENTITY;
    }
    Quat::from_rotation_arc(from.normalize(), to.normalize())
}

/// How a mark on a rotation ring is drawn: every quarter turn longest, every eighth a little shorter than
/// that, all the others the same short length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TickKind {
    Regular,
    /// A multiple of 45 degrees that is not one of 90.
    Eighth,
    /// A multiple of 90 degrees.
    Quarter,
}

impl TickKind {
    /// How far the mark reaches out from the ring, as a fraction of the ring's radius.
    pub fn length(self) -> f32 {
        match self {
            TickKind::Regular => 0.07,
            TickKind::Eighth => 0.115,
            TickKind::Quarter => 0.17,
        }
    }
}

/// The marks of a rotation ring: angles in degrees measured from where the drag started, and their kind.
/// The spacing is the rotation increment (with snapping off, 15 degrees); marks never come closer than a few
/// screen points, so a small ring is not a solid band (the spacing then grows through values that divide
/// 90). Every multiple of 45 degrees is marked, whatever the increment, so the landmarks are always there.
pub fn tick_marks(increment_degrees: f32, ring_on_screen: f32) -> Vec<(f32, TickKind)> {
    const MIN_GAP_ON_SCREEN: f32 = 7.0;
    let mut step = if increment_degrees >= 1.0 { increment_degrees } else if increment_degrees > 0.0 { 1.0 } else { 15.0 };
    let densest = (360.0 * MIN_GAP_ON_SCREEN / (std::f32::consts::TAU * ring_on_screen.max(1.0))).max(1.0);
    if step < densest {
        step = [1.0, 3.0, 5.0, 9.0, 15.0, 45.0, 90.0].into_iter().find(|s| *s >= densest).unwrap_or(90.0);
    }
    let count = (360.0 / step + 1e-3).floor() as i32;
    let mut angles: Vec<f32> = (0..count).map(|i| i as f32 * step).collect();
    for k in 0..8 {
        let landmark = k as f32 * 45.0;
        if !angles.iter().any(|a| (a - landmark).abs() < 1e-3) {
            angles.push(landmark);
        }
    }
    angles.sort_by(f32::total_cmp);
    angles
        .into_iter()
        .map(|a| {
            let kind = if (a % 90.0).abs() < 1e-3 {
                TickKind::Quarter
            } else if (a % 45.0).abs() < 1e-3 {
                TickKind::Eighth
            } else {
                TickKind::Regular
            };
            (a, kind)
        })
        .collect()
}

/// An angle (radians) brought into (-pi, pi]: the short way round.
pub fn wrap_pi(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let a = angle.rem_euclid(tau);
    if a > std::f32::consts::PI {
        a - tau
    } else {
        a
    }
}

/// Follows an angle that is only known modulo a full turn (the angle between two directions): returns
/// the total turned so far, given the total before (`total`), the raw angle before (`previous_raw`) and the
/// raw angle now. Each step is the short way round, so the total passes 180 and 360 degrees without
/// jumping, and keeps counting.
pub fn unwrap_angle(total: f32, previous_raw: f32, raw: f32) -> f32 {
    total + wrap_pi(raw - previous_raw)
}

/// How a ruler is laid out along its axis (positions relative to where the drag started): the stretch
/// travelled, and the two stretches of guide before and after it. They only touch at their ends, so no
/// two lines are ever drawn on top of one another (which flickers whatever the depth bias).
pub struct RulerSpans {
    pub guide: [(f32, f32); 2],
    pub travelled: (f32, f32),
}

pub fn ruler_spans(travelled: f32, reach: f32) -> RulerSpans {
    let (lo, hi) = (travelled.min(0.0) - reach, travelled.max(0.0) + reach);
    RulerSpans { guide: [(lo, travelled.min(0.0)), (travelled.max(0.0), hi)], travelled: (0.0, travelled) }
}

/// Positions of the 5-unit marks along a line from a start point: every multiple of `spacing`
/// within `reach` of the start (the start itself is mark 0).
pub fn ruler_marks(travelled: f32, spacing: f32, extra: f32) -> Vec<f32> {
    let lo = travelled.min(0.0) - extra;
    let hi = travelled.max(0.0) + extra;
    let first = (lo / spacing).ceil() as i32;
    let last = (hi / spacing).floor() as i32;
    // Never more than a few hundred marks, whatever the distance.
    let stride = (((last - first).max(1) as f32) / 200.0).ceil().max(1.0) as i32;
    (first..=last).filter(|k| k % stride == 0).map(|k| k as f32 * spacing).collect()
}

/// How long the handles are drawn: constant on screen, whatever the distance.
pub fn handle_len(camera: Vec3, origin: Vec3) -> f32 {
    ((origin - camera).length() * 0.18).clamp(2.0, 60.0)
}

/// How close (in studs, at the handle) the mouse ray has to pass to count as a hit.
fn tolerance(camera: Vec3, origin: Vec3) -> f32 {
    ((origin - camera).length() * 0.025).max(0.15)
}

/// Closest points between a ray (o, d) and an infinite line (q, e): `(s, t)` are the parameters.
pub fn ray_line(o: Vec3, d: Vec3, q: Vec3, e: Vec3) -> Option<(f32, f32)> {
    let w0 = o - q;
    let (a, b, c) = (d.dot(d), d.dot(e), e.dot(e));
    let (dd, ee) = (d.dot(w0), e.dot(w0));
    let denom = a * c - b * b;
    if denom.abs() < 1e-6 {
        return None;
    }
    Some(((b * ee - c * dd) / denom, (a * ee - b * dd) / denom))
}

/// Where a ray meets the plane through `origin` with normal `normal`.
pub fn ray_plane(ray: (Vec3, Vec3), origin: Vec3, normal: Vec3) -> Option<Vec3> {
    ray_plane_facing(ray, origin, normal, 1e-5)
}

/// Like [`ray_plane`], but not for a plane the ray runs nearly along: `min_facing` is the least cosine
/// between the ray and the plane's normal. Where such a ray meets the plane is far away and moves a long way
/// for a tiny movement of the mouse, which would throw a dragged object across the scene.
pub fn ray_plane_facing(ray: (Vec3, Vec3), origin: Vec3, normal: Vec3, min_facing: f32) -> Option<Vec3> {
    let denom = ray.1.dot(normal);
    if denom.abs() < min_facing.max(1e-5) {
        return None;
    }
    let t = (origin - ray.0).dot(normal) / denom;
    (t > 0.0).then(|| ray.0 + ray.1 * t)
}

/// Where a mouse ray meets the plane of a move handle that is being dragged (see [`MIN_PLANE_FACING`]).
pub fn ray_handle_plane(ray: (Vec3, Vec3), origin: Vec3, normal: Vec3) -> Option<Vec3> {
    ray_plane_facing(ray, origin, normal, MIN_PLANE_DRAG_FACING)
}

/// The plane an object picked up with the Select tool slides on: the horizontal plane through where it was
/// grabbed, unless the view is nearly level (a horizontal plane is then seen edge-on, and the object would
/// fly off with the slightest movement), when it is the plane facing the camera, which keeps the grabbed
/// point at the same depth. Chosen once, from the ray that grabbed it.
pub fn surface_drag_normal(grab_ray_dir: Vec3) -> Vec3 {
    if grab_ray_dir.y.abs() > 0.12 {
        Vec3::Y
    } else {
        -grab_ray_dir.normalize_or_zero()
    }
}

pub fn ring_radius(camera: Vec3, origin: Vec3) -> f32 {
    handle_len(camera, origin) * 0.9
}

/// The rotation ring under the mouse ray, if any.
pub fn hit_ring(camera: Vec3, ray: (Vec3, Vec3), origin: Vec3, dirs: &[Vec3; 3]) -> Option<usize> {
    let radius = ring_radius(camera, origin);
    let tol = tolerance(camera, origin) * 1.5;
    let mut best: Option<(usize, f32)> = None;
    for (i, axis) in dirs.iter().enumerate() {
        let Some(p) = ray_plane(ray, origin, *axis) else { continue };
        let off_ring = ((p - origin).length() - radius).abs();
        let along = (p - ray.0).length();
        if off_ring < tol && best.is_none_or(|(_, d)| along < d) {
            best = Some((i, along));
        }
    }
    best.map(|(i, _)| i)
}

/// The signed angle (radians, right-handed around `axis`) that turns `from` into `to`.
pub fn signed_angle(axis: Vec3, from: Vec3, to: Vec3) -> f32 {
    axis.dot(from.cross(to)).atan2(from.dot(to))
}

/// Rounds `value` to a multiple of `step` (`step <= 0` leaves it alone).
pub fn snap(value: f32, step: f32) -> f32 {
    if step > 0.0 {
        (value / step).round() * step
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ray(from: Vec3, to: Vec3) -> (Vec3, Vec3) {
        (from, (to - from).normalize())
    }

    #[test]
    fn world_and_local_axes() {
        let turned = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        assert_eq!(axis_dirs(Tool::Move, Space::World, turned), AXES);
        let local = axis_dirs(Tool::Move, Space::Local, turned);
        assert!(local[0].abs_diff_eq(Vec3::NEG_Z, 1e-5), "local X of a part turned 90 degrees around Y points along -Z");
        assert!(axis_dirs(Tool::Scale, Space::World, turned)[0].abs_diff_eq(Vec3::NEG_Z, 1e-5), "scale always uses the part's own axes");
    }

    #[test]
    fn the_rotation_ring_under_the_mouse_is_found() {
        let cam = Vec3::new(0.0, 0.0, 30.0);
        let r = ring_radius(cam, Vec3::ZERO);
        // The Z ring (a circle in the XY plane) passes through (r, 0, 0) as seen from the camera.
        assert_eq!(hit_ring(cam, ray(cam, Vec3::new(r, 0.0, 0.0)), Vec3::ZERO, &AXES), Some(2));
        assert_eq!(hit_ring(cam, ray(cam, Vec3::new(r * 0.2, 0.0, 0.0)), Vec3::ZERO, &AXES), None, "inside the ring is not a hit");
    }

    #[test]
    fn dragging_around_a_ring_gives_the_swept_angle() {
        let v0 = Vec3::X;
        let quarter = Vec3::Y;
        assert!((signed_angle(Vec3::Z, v0, quarter) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((signed_angle(Vec3::Z, quarter, v0) + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((signed_angle(Vec3::NEG_Z, v0, quarter) + std::f32::consts::FRAC_PI_2).abs() < 1e-5, "the sense depends on the axis direction");
    }

    #[test]
    fn rays_meet_planes_only_in_front_of_the_camera() {
        let p = ray_plane(ray(Vec3::new(0.0, 5.0, 0.0), Vec3::new(1.0, 0.0, 0.0)), Vec3::ZERO, Vec3::Y).unwrap();
        assert!(p.abs_diff_eq(Vec3::new(1.0, 0.0, 0.0), 1e-5));
        assert!(ray_plane((Vec3::new(0.0, 5.0, 0.0), Vec3::Y), Vec3::ZERO, Vec3::Y).is_none(), "the plane is behind the ray");
        assert!(ray_plane((Vec3::new(0.0, 5.0, 0.0), Vec3::X), Vec3::ZERO, Vec3::Y).is_none(), "a ray parallel to the plane never meets it");
    }

    fn view() -> View {
        View { pos: Vec3::new(0.0, 5.0, 30.0), rot: Quat::from_rotation_x(-0.15), fov: 70f32.to_radians(), min: bevy::math::Vec2::new(250.0, 60.0), size: bevy::math::Vec2::new(900.0, 700.0) }
    }

    /// A view from `cam` looking at `target`.
    fn look(cam: Vec3, target: Vec3) -> View {
        let t = bevy::prelude::Transform::from_translation(cam).looking_at(target, Vec3::Y);
        View { pos: cam, rot: t.rotation, fov: 70f32.to_radians(), min: Vec2::ZERO, size: Vec2::new(1000.0, 700.0) }
    }

    /// What the move gizmo at `origin` picks when the mouse is on `world`, seen from `cam`.
    fn move_hit(cam: Vec3, origin: Vec3, world: Vec3) -> Option<MoveHandle> {
        let v = look(cam, origin);
        hit_move(&v, v.project(world)?, origin, &AXES)
    }

    #[test]
    fn projecting_and_casting_rays_are_inverses() {
        let v = view();
        for p in [Vec3::new(0.0, 0.0, 0.0), Vec3::new(8.0, 3.0, -10.0), Vec3::new(-12.0, 9.0, 5.0)] {
            let screen = v.project(p).expect("in front of the camera");
            let (o, d) = v.ray(screen).unwrap();
            let closest = o + d * (p - o).dot(d);
            assert!((closest - p).length() < 1e-3, "{p:?}: the ray through its own screen position passes through it");
        }
        assert!(v.project(Vec3::new(0.0, 5.0, 60.0)).is_none(), "behind the camera");
        let centre = v.project(v.pos + v.forward() * 10.0).unwrap();
        assert!((centre - (v.min + v.size * 0.5)).length() < 1e-2, "straight ahead is the middle of the viewport");
    }

    #[test]
    fn the_move_tool_has_six_arrows_and_three_planes_on_the_side_facing_you() {
        let cam = Vec3::new(10.0, 12.0, 30.0);
        let origin = Vec3::ZERO;
        let len = handle_len(cam, origin);
        // Each of the six arrows can be picked, in both directions.
        for (i, axis) in AXES.iter().enumerate() {
            for sign in [1.0f32, -1.0] {
                let target = *axis * (sign * len * 0.6);
                assert_eq!(move_hit(cam, origin, target), Some(MoveHandle::Arrow(i, sign)), "axis {i} sign {sign}");
            }
        }
        // The planes sit towards the camera: here +X, +Y and +Z all face it.
        let signs = facing_signs(cam, origin, &AXES);
        assert_eq!(signs, [1.0, 1.0, 1.0]);
        for normal in 0..3 {
            let c = plane_corners(origin, &AXES, &signs, normal, len);
            let middle = (c[0] + c[2]) * 0.5;
            assert_eq!(move_hit(cam, origin, middle), Some(MoveHandle::Plane(normal)), "plane {normal}");
        }
        // Seen from the other side the planes move to the other side of the axes.
        let behind = Vec3::new(-10.0, -12.0, -30.0);
        assert_eq!(facing_signs(behind, origin, &AXES), [-1.0, -1.0, -1.0]);
        let flipped = plane_corners(origin, &AXES, &[-1.0, -1.0, -1.0], 2, len);
        assert!(flipped[0].x < 0.0 && flipped[0].y < 0.0, "the XY plane is now in the -X -Y quarter");
        assert_eq!(move_hit(behind, origin, (flipped[0] + flipped[2]) * 0.5), Some(MoveHandle::Plane(2)));
    }

    #[test]
    fn plane_handles_name_the_axes_they_move() {
        assert_eq!(plane_axes(0), (1, 2));
        assert_eq!(plane_axes(1), (0, 2));
        assert_eq!(plane_axes(2), (0, 1));
    }

    #[test]
    fn scale_spheres_sit_on_the_faces_of_the_oriented_bounding_box() {
        let turned = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let b = BoxBounds { center: Vec3::new(10.0, 2.0, 0.0), half: Vec3::new(4.0, 1.0, 2.0), rot: turned };
        assert!(b.face(0, 1.0).abs_diff_eq(Vec3::new(10.0, 2.0, -4.0), 1e-4), "the +X face of a part turned 90 degrees around Y faces -Z");
        assert!(b.face(1, -1.0).abs_diff_eq(Vec3::new(10.0, 1.0, 0.0), 1e-4));
        assert!(b.outward(2, -1.0).abs_diff_eq(Vec3::X, 1e-4) || b.outward(2, -1.0).abs_diff_eq(Vec3::NEG_X, 1e-4));

        let cam = Vec3::new(10.0, 40.0, 0.0);
        let hit = hit_scale(cam, ray(cam, b.handle(cam, 1, 1.0)), &b);
        assert_eq!(hit, Some((1, 1.0)), "aiming at the top face's sphere picks it");
        assert_eq!(hit_scale(cam, ray(cam, Vec3::new(40.0, 2.0, 0.0)), &b), None, "aiming at empty space picks nothing");
        // Bigger objects get the same on-screen handle, not bigger ones.
        let far = Vec3::new(0.0, 0.0, 100.0);
        assert!(sphere_radius(Vec3::ZERO, far) > sphere_radius(Vec3::ZERO, Vec3::new(0.0, 0.0, 10.0)));
    }

    #[test]
    fn the_trackball_turns_the_way_the_mouse_moves() {
        let origin = Vec3::ZERO;
        let r = 3.0;
        let cam = Vec3::new(0.0, 0.0, 20.0);
        let a = trackball_point(ray(cam, Vec3::new(0.0, 0.0, 3.0)), origin, r);
        assert!(a.abs_diff_eq(Vec3::new(0.0, 0.0, 3.0), 1e-3), "the centre of the ball faces the camera");
        let b = trackball_point(ray(cam, Vec3::new(2.0, 0.0, 0.0)), origin, r);
        let q = trackball_rotation(a, b);
        // Dragging from the middle to the right turns the front of the ball to the right (around +Y).
        assert!((q * Vec3::Z).x > 0.5);
        let outside = trackball_point(ray(cam, Vec3::new(9.0, 0.0, 0.0)), origin, r);
        assert!((outside.length() - r).abs() < 1e-3, "past the edge the point stays on the ball's outline");
        assert!(inside_trackball(ray(cam, Vec3::new(1.0, 1.0, 0.0)), origin, r));
        assert!(!inside_trackball(ray(cam, Vec3::new(5.0, 0.0, 0.0)), origin, r));
        assert!(!inside_trackball((cam, Vec3::Z), origin, r), "not when the ball is behind the camera");
        assert_eq!(trackball_rotation(Vec3::ZERO, Vec3::X), Quat::IDENTITY, "degenerate input does nothing");
    }

    #[test]
    fn rings_are_marked_at_the_rotation_increment_with_longer_marks_at_the_eighths_and_quarters() {
        let big = 400.0; // a ring this many points round
        let marks = |step: f32, ring: f32| tick_marks(step, ring);
        let angles = |m: &[(f32, TickKind)]| m.iter().map(|(a, _)| *a).collect::<Vec<_>>();
        assert_eq!(angles(&marks(90.0, big)), vec![0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0], "the eighths are always marked");
        assert_eq!(marks(15.0, big).len(), 24);
        assert_eq!(marks(0.0, big).len(), 24, "with snapping off the ring is marked every 15 degrees");
        assert_eq!(marks(5.0, big).len(), 72);
        let kinds = marks(15.0, big);
        for (angle, kind) in &kinds {
            let expect = if angle % 90.0 == 0.0 {
                TickKind::Quarter
            } else if angle % 45.0 == 0.0 {
                TickKind::Eighth
            } else {
                TickKind::Regular
            };
            assert_eq!(*kind, expect, "{angle}");
        }
        assert_eq!(kinds.iter().filter(|(_, k)| *k == TickKind::Quarter).count(), 4);
        assert_eq!(kinds.iter().filter(|(_, k)| *k == TickKind::Eighth).count(), 4);
        // Longest at the quarters, a little shorter at the eighths, shortest elsewhere.
        assert!(TickKind::Quarter.length() > TickKind::Eighth.length() && TickKind::Eighth.length() > TickKind::Regular.length());
        // An increment that does not divide 45 still gets its landmarks (and an even spacing in between).
        let ten = marks(10.0, big);
        assert!(ten.contains(&(45.0, TickKind::Eighth)) && ten.contains(&(90.0, TickKind::Quarter)) && ten.contains(&(270.0, TickKind::Quarter)));
    }

    #[test]
    fn marks_on_a_small_ring_never_crowd_into_a_solid_band() {
        for ring in [20.0f32, 40.0, 80.0, 160.0, 400.0] {
            let marks = tick_marks(1.0, ring);
            let degrees_between = marks.windows(2).map(|w| w[1].0 - w[0].0).fold(f32::MAX, f32::min);
            let points_between = degrees_between.to_radians() * ring;
            assert!(points_between >= 6.0 || degrees_between >= 45.0, "a ring {ring} points round has marks {points_between:.1} points apart");
            assert!(marks.iter().any(|(a, k)| *a == 90.0 && *k == TickKind::Quarter), "but never without its quarter turns");
        }
        assert_eq!(tick_marks(0.25, 1000.0).len(), 360, "on a big ring a degree is as fine as it gets");
    }

    #[test]
    fn the_angle_of_a_ring_is_counted_through_half_and_whole_turns() {
        use std::f32::consts::PI;
        // The raw angle between two directions flips from +pi to -pi: the total goes on counting.
        let mut total = 0.0f32;
        let mut raw = 0.0f32;
        for k in 1..=40 {
            let real = k as f32 * 20.0f32.to_radians();
            let now = wrap_pi(real);
            total = unwrap_angle(total, raw, now);
            raw = now;
            assert!((total - real).abs() < 1e-4, "{k}: {} vs {}", total.to_degrees(), real.to_degrees());
        }
        assert!((total.to_degrees() - 800.0).abs() < 1e-2, "two and a bit turns");
        // And in the other direction.
        let (mut total, mut raw) = (0.0f32, 0.0f32);
        for k in 1..=20 {
            let real = -(k as f32) * 25.0f32.to_radians();
            let now = wrap_pi(real);
            total = unwrap_angle(total, raw, now);
            raw = now;
        }
        assert!((total.to_degrees() + 500.0).abs() < 1e-2);
        assert!(wrap_pi(PI).abs() > PI - 1e-4 && wrap_pi(3.0 * PI).abs() > PI - 1e-3 && wrap_pi(2.0 * PI).abs() < 1e-5 && wrap_pi(0.5).abs() - 0.5 < 1e-6, "the short way round, ±pi at the far side");
    }

    #[test]
    fn a_polygon_is_hit_inside_and_measured_outside() {
        let square = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let reversed: Vec<Vec2> = square.iter().rev().copied().collect();
        for poly in [&square[..], &reversed[..]] {
            assert_eq!(point_polygon_distance(Vec2::new(5.0, 5.0), poly), 0.0);
            assert!((point_polygon_distance(Vec2::new(14.0, 5.0), poly) - 4.0).abs() < 1e-5);
            assert!((point_polygon_distance(Vec2::new(13.0, 14.0), poly) - 5.0).abs() < 1e-5, "past a corner it is the distance to the corner");
        }
        assert!((point_segment_distance(Vec2::new(5.0, 3.0), Vec2::ZERO, Vec2::new(10.0, 0.0)) - 3.0).abs() < 1e-5);
        assert_eq!(point_segment_distance(Vec2::new(3.0, 4.0), Vec2::ZERO, Vec2::ZERO), 5.0, "a degenerate segment is a point");
    }

    #[test]
    fn a_plane_ray_that_runs_along_the_plane_meets_nothing() {
        let r = (Vec3::new(0.0, 10.0, 0.0), Vec3::new(1.0, -0.01, 0.0).normalize());
        assert!(ray_plane(r, Vec3::ZERO, Vec3::Y).is_some(), "the plain version still meets it, a very long way off");
        assert!(ray_handle_plane(r, Vec3::ZERO, Vec3::Y).is_none(), "a drag does not follow it there");
        assert!(ray_handle_plane((Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.3, -1.0, 0.0).normalize()), Vec3::ZERO, Vec3::Y).is_some());
    }

    #[test]
    fn the_ruler_has_a_mark_every_five_units_around_the_distance_travelled() {
        assert_eq!(ruler_marks(12.0, 5.0, 5.0), vec![-5.0, 0.0, 5.0, 10.0, 15.0]);
        assert_eq!(ruler_marks(-7.0, 5.0, 0.0), vec![-5.0, 0.0]);
        assert!(ruler_marks(0.0, 5.0, 5.0).contains(&0.0));
        assert!(ruler_marks(1.0e6, 5.0, 5.0).len() <= 201, "a huge drag does not produce a huge number of marks");
    }

    #[test]
    fn the_arrows_start_a_gap_away_from_the_centre_and_end_in_a_cone() {
        let cam = Vec3::new(25.0, 25.0, 25.0);
        let len = handle_len(cam, Vec3::ZERO);
        // Aiming at the middle of the gizmo grabs nothing: the arrows start a gap away.
        assert_eq!(move_hit(cam, Vec3::ZERO, Vec3::ZERO), None, "the gap is empty");
        // ... but just past it does, and so does the very tip.
        assert_eq!(move_hit(cam, Vec3::ZERO, Vec3::X * (len * (ARROW_GAP + 0.1))), Some(MoveHandle::Arrow(0, 1.0)));
        assert_eq!(move_hit(cam, Vec3::ZERO, Vec3::X * (len * 0.97)), Some(MoveHandle::Arrow(0, 1.0)));
        assert!(ARROW_GAP < PLANE_NEAR, "the planes sit outside the gap");
        // The cone has a tip at the end of the arrow and a base circle behind it.
        let pts = cone_points(Vec3::ZERO, Vec3::Z * 2.0, 1.0, 8);
        assert_eq!(pts.len(), 9);
        assert_eq!(*pts.last().unwrap(), Vec3::Z * 2.0);
        assert!(pts[..8].iter().all(|p| (p.z).abs() < 1e-5 && ((p.x * p.x + p.y * p.y).sqrt() - 1.0).abs() < 1e-4));
    }

    #[test]
    fn the_convex_hull_is_the_outline_of_the_projected_cone() {
        use bevy::math::Vec2;
        let square_with_inside = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0), Vec2::new(2.0, 2.0), Vec2::new(1.0, 3.0)];
        let hull = convex_hull(&square_with_inside);
        assert_eq!(hull.len(), 4, "the points inside are not on the outline");
        assert!(hull.contains(&Vec2::new(0.0, 0.0)) && hull.contains(&Vec2::new(4.0, 4.0)));
        assert_eq!(convex_hull(&[Vec2::ZERO, Vec2::X]).len(), 2, "too few points: returned as they are");
    }

    #[test]
    fn scale_spheres_stand_off_from_the_faces_and_the_union_box_covers_all() {
        let b = BoxBounds { center: Vec3::ZERO, half: Vec3::new(2.0, 1.0, 1.0), rot: Quat::IDENTITY };
        let cam = Vec3::new(0.0, 0.0, 30.0);
        let h = b.handle(cam, 0, 1.0);
        assert!(h.x > b.face(0, 1.0).x + 0.1, "outside the face");
        assert!(h.y.abs() < 1e-4 && h.z.abs() < 1e-4);
        // The sphere that is drawn is the one that can be picked.
        assert_eq!(hit_scale(cam, ray(cam, h), &b), Some((0, 1.0)));
        let (c, half) = union_box([(Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 2.0)), (Vec3::new(5.0, -4.0, 0.0), Vec3::new(9.0, 0.0, 1.0))]).unwrap();
        assert_eq!((c, half), (Vec3::new(4.0, -1.0, 1.0), Vec3::new(5.0, 3.0, 1.0)));
        assert!(union_box(std::iter::empty()).is_none());
    }

    #[test]
    fn snapping() {
        assert_eq!(snap(1.4, 1.0), 1.0);
        assert_eq!(snap(1.6, 1.0), 2.0);
        assert!((snap(0.37, 0.25) - 0.25).abs() < 1e-6);
        assert_eq!(snap(0.37, 0.0), 0.37, "no step means free");
        assert!((snap(40.0, 15.0) - 45.0).abs() < 1e-5);
    }
}
