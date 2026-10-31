//! Spatial queries against the simulated world: ray casts, shape casts and overlap tests.
//!
//! They see the parts the simulation holds (so a part created a moment ago is there: the queries bring the world
//! up to date first), and only what a part allows: `CanQuery` off hides it from all of them, `RespectCanCollide`
//! hides what does not collide, and the filter of a [`QueryParams`] (instances to leave out or to include alone,
//! a collision group) decides the rest. Hits are reported per part, nearest first.

use bevy::math::{Quat, Vec3};
use box3d::{QueryFilter, ShapeProxy};

use super::{
    box3d_error,
    bridge::{bv, check_finite, fv},
    shapes, ColliderShape, PhysicsBridge,
};
use crate::{core::Id, dm::DataModel};

/// How far a part may be from another and still count as touching it (studs).
const TOUCH_MARGIN: f32 = 0.02;

/// Which parts a query considers.
#[derive(Clone, Debug, Default)]
pub struct QueryParams {
    /// Instances (and everything inside them) the filter applies to.
    pub filter: Vec<Id>,
    /// `false`: the instances are left out. `true`: only they (and what is inside them) are considered.
    pub include: bool,
    /// The query behaves as a part in this collision group would: it only sees what that group collides with.
    pub group: Option<String>,
    /// Leave out the parts that do not collide (`CanCollide` off).
    pub respect_can_collide: bool,
    /// At most this many parts (overlap queries); 0 is no limit.
    pub max_parts: usize,
}

/// What a ray or shape cast hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub id: Id,
    pub position: Vec3,
    pub normal: Vec3,
    /// How far along the cast the hit is, in studs.
    pub distance: f32,
}

/// The solid a shape cast sweeps.
#[derive(Clone, Copy, Debug)]
pub enum CastShape {
    Sphere { radius: f32 },
    Block { size: Vec3, rotation: Quat },
}

impl PhysicsBridge {
    fn query_filter(&mut self, params: &QueryParams) -> Result<QueryFilter, String> {
        let (category, mask) = match &params.group {
            Some(name) => {
                let (c, m, known) = self.groups.filter_for(name);
                if !known {
                    return Err(format!("'{name}' is not a registered collision group"));
                }
                (c, m)
            }
            None => (u64::MAX, u64::MAX),
        };
        Ok(QueryFilter { category_bits: category, mask_bits: mask, id: 0 })
    }

    /// Whether the part is one the query considers.
    fn accepts(&self, dm: &DataModel, params: &QueryParams, id: Id) -> bool {
        let Some(rec) = self.parts.get(&id) else { return false };
        if !rec.applied.can_query || (params.respect_can_collide && !rec.applied.can_collide) {
            return false;
        }
        if params.filter.is_empty() {
            return !params.include;
        }
        let listed = params.filter.iter().any(|f| *f == id || dm.is_descendant_of(id, *f));
        listed == params.include
    }

    /// The nearest part along a ray: `direction` is both the way it goes and how far (like Roblox).
    pub fn raycast(&mut self, dm: &mut DataModel, origin: Vec3, direction: Vec3, params: &QueryParams) -> Result<Option<Hit>, String> {
        check_finite("origin", origin)?;
        check_finite("direction", direction)?;
        let length = direction.length();
        if length < 1e-6 {
            return Ok(None);
        }
        self.flush(dm)?;
        let world = self.world()?;
        let filter = self.query_filter(params)?;
        let mut best: Option<Hit> = None;
        world.cast_ray(bv(origin), bv(direction), filter, |hit| {
            let Some(id) = self.part_of(hit.shape.handle()).filter(|id| self.accepts(dm, params, *id)) else { return -1.0 };
            best = Some(Hit { id, position: fv(hit.point), normal: fv(hit.normal), distance: hit.fraction * length });
            hit.fraction // nothing farther can be the nearest
        });
        Ok(best)
    }

    /// The nearest part a solid hits as it moves along `direction` from `origin`.
    pub fn shapecast(&mut self, dm: &mut DataModel, origin: Vec3, shape: CastShape, direction: Vec3, params: &QueryParams) -> Result<Option<Hit>, String> {
        check_finite("origin", origin)?;
        check_finite("direction", direction)?;
        let (points, radius) = cast_proxy(shape)?;
        let length = direction.length();
        if length < 1e-6 {
            return Ok(None);
        }
        self.flush(dm)?;
        let world = self.world()?;
        let filter = self.query_filter(params)?;
        let points: Vec<box3d::Vec3> = points.into_iter().map(bv).collect();
        let proxy = ShapeProxy::new(&points, radius).map_err(|e| format!("cannot cast that shape: {}", box3d_error(e)))?;
        let mut best: Option<Hit> = None;
        world.cast_shape(bv(origin), proxy, bv(direction), filter, |hit| {
            let Some(id) = self.part_of(hit.shape.handle()).filter(|id| self.accepts(dm, params, *id)) else { return -1.0 };
            best = Some(Hit { id, position: fv(hit.point), normal: fv(hit.normal), distance: hit.fraction * length });
            hit.fraction
        });
        Ok(best)
    }

    /// The parts that overlap a solid placed at `origin`.
    pub fn overlap(&mut self, dm: &mut DataModel, origin: Vec3, shape: CastShape, params: &QueryParams) -> Result<Vec<Id>, String> {
        self.overlap_inner(dm, origin, shape, params, None, 0.0)
    }

    /// The parts that overlap a part (never the part itself).
    pub fn overlap_part(&mut self, dm: &mut DataModel, part: Id, params: &QueryParams) -> Result<Vec<Id>, String> {
        let Some(info) = dm.collider(part) else { return Err(format!("{} is not a part", dm.name(part))) };
        let (points, radius) = shapes::query_proxy(info.shape, info.size, info.rotation);
        self.overlap_points(dm, info.center, points, radius, params, Some(part), 0.0)
    }

    /// The parts a part is touching: within [`TOUCH_MARGIN`] of it, including the parts that do not collide
    /// (a sensor it is inside, or inside it) and leaving out those that cannot be touched.
    pub fn touching_parts(&mut self, dm: &mut DataModel, part: Id) -> Result<Vec<Id>, String> {
        let Some(info) = dm.collider(part) else { return Err(format!("{} is not a part", dm.name(part))) };
        if !info.can_touch {
            return Ok(Vec::new());
        }
        let (points, radius) = shapes::query_proxy(info.shape, info.size, info.rotation);
        let params = QueryParams::default();
        let found = self.overlap_points(dm, info.center, points, radius, &params, Some(part), TOUCH_MARGIN)?;
        Ok(found.into_iter().filter(|o| self.parts.get(o).is_some_and(|r| r.applied.can_touch)).collect())
    }

    fn overlap_inner(&mut self, dm: &mut DataModel, origin: Vec3, shape: CastShape, params: &QueryParams, skip: Option<Id>, margin: f32) -> Result<Vec<Id>, String> {
        check_finite("position", origin)?;
        let (points, radius) = cast_proxy(shape)?;
        self.overlap_points(dm, origin, points, radius, params, skip, margin)
    }

    #[allow(clippy::too_many_arguments)]
    fn overlap_points(&mut self, dm: &mut DataModel, origin: Vec3, points: Vec<Vec3>, radius: f32, params: &QueryParams, skip: Option<Id>, margin: f32) -> Result<Vec<Id>, String> {
        self.flush(dm)?;
        let world = self.world()?;
        let filter = self.query_filter(params)?;
        let points: Vec<box3d::Vec3> = points.into_iter().map(bv).collect();
        let proxy = ShapeProxy::new(&points, radius + margin).map_err(|e| format!("cannot test that shape: {}", box3d_error(e)))?;
        let mut found: Vec<Id> = Vec::new();
        world.overlap_shape(bv(origin), proxy, filter, |hit| {
            if let Some(id) = self.part_of(hit.handle()).filter(|id| Some(*id) != skip && self.accepts(dm, params, *id)) {
                if !found.contains(&id) {
                    found.push(id);
                }
            }
            params.max_parts == 0 || found.len() < params.max_parts
        });
        found.sort_unstable(); // (the same order on every run, not the order the world found them in)
        Ok(found)
    }
}

/// The points and radius Box3D takes for a solid that is cast or tested.
fn cast_proxy(shape: CastShape) -> Result<(Vec<Vec3>, f32), String> {
    match shape {
        CastShape::Sphere { radius } => {
            if !(radius.is_finite() && radius > 0.0) {
                return Err("the radius must be a positive number".into());
            }
            Ok((vec![Vec3::ZERO], radius))
        }
        CastShape::Block { size, rotation } => {
            check_finite("size", size)?;
            if size.min_element() <= 0.0 {
                return Err("every side of the block must be positive".into());
            }
            Ok(shapes::query_proxy(ColliderShape::Box, size, rotation))
        }
    }
}
