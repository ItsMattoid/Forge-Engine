//! Spatial queries and operations on instances that have a `Transform`: world-space
//! CFrames (including transforms relative to a parent), pivots, and the collision
//! volume described by a collider component.

use bevy::math::{Quat, Vec3};

use super::{DataModel, D_X};
use crate::{
    core::{registry, Id, Value},
    physics::ColliderInfo,
};

/// The sizes a part may have (the `Size` property's own limits).
pub const PART_SIZE_RANGE: (f32, f32) = (0.001, 100_000.0);

impl DataModel {
    // ---- CFrame & size ------------------------------------------------------------------------

    /// The stored `CFrame` property (local to the parent for relative classes).
    pub fn cframe(&self, id: Id) -> Option<(Vec3, Quat)> {
        match self.get(id)?.raw("CFrame")? {
            Value::CF(p, q) => Some((*p, *q)),
            _ => None,
        }
    }

    /// The CFrame in world space (applies the parent's transform for relative classes).
    pub fn world_cframe(&self, id: Id) -> Option<(Vec3, Quat)> {
        let inst = self.get(id)?;
        let (p, q) = self.cframe(id)?;
        if inst.class.relative_transform {
            if let Some((pp, pq)) = inst.parent.and_then(|par| self.get(par).filter(|i| i.class.has_component("Transform")).and_then(|_| self.world_cframe(par))) {
                return Some((pp + pq * p, (pq * q).normalize()));
            }
        }
        Some((p, q))
    }

    pub fn size(&self, id: Id) -> Vec3 {
        match self.get(id).and_then(|i| i.raw("Size")) {
            Some(Value::V3(v)) => *v,
            _ => Vec3::ONE,
        }
    }

    pub fn set_cframe(&mut self, id: Id, p: Vec3, q: Quat) {
        let _ = self.set_prop(id, "CFrame", Value::CF(p, q));
    }

    // ---- dependents & collision volume ----------------------------------------------------------

    /// Marks the instances whose transform is relative to `id` (e.g. lights attached to a part).
    pub fn mark_dependents(&mut self, id: Id) {
        // The common case (a part with no attached lights) leaves here without allocating.
        let has_relative = self.get(id).is_some_and(|i| i.children.iter().any(|k| self.get(*k).is_some_and(|c| c.class.relative_transform)));
        if !has_relative {
            return;
        }
        for k in self.children(id) {
            if self.get(k).is_some_and(|i| i.class.relative_transform) {
                // Only the position changed: attached lights just move, they are not rebuilt.
                self.mark(k, D_X);
                self.mark_dependents(k);
            }
        }
    }

    /// The collision volume described by the instance's collider component, if any.
    pub fn collider(&self, id: Id) -> Option<ColliderInfo> {
        let class = self.get(id)?.class;
        class.components.iter().find_map(|c| c.collider.and_then(|f| f(self, id)))
    }

    /// All instances that have a component providing a collider.
    pub fn colliders(&self) -> Vec<Id> {
        let mut out = Vec::new();
        for name in registry().collider_components() {
            out.extend(self.with_component(name));
        }
        out
    }

    // ---- pivots (generic: any instance with a transform, or a PrimaryPart reference) -----------

    /// Where an object turns and moves from: its own position (or, for a model, its primary part's
    /// pivot) shifted by its `PivotOffset`, which is measured in the object's own axes.
    pub fn get_pivot(&self, id: Id) -> Option<(Vec3, Quat)> {
        let (p, q) = self.pivot_base(id)?;
        Some((p + q * self.vec3(id, "PivotOffset"), q))
    }

    /// The pivot before the object's own `PivotOffset` is applied.
    pub fn pivot_base(&self, id: Id) -> Option<(Vec3, Quat)> {
        let inst = self.get(id)?;
        if inst.class.has_component("Transform") {
            return self.world_cframe(id);
        }
        if let Some(part) = inst.class.prop("PrimaryPart").and_then(|_| self.reference(id, "PrimaryPart")) {
            if let Some(pivot) = self.get_pivot(part) {
                return Some(pivot);
            }
        }
        let first = self.subtree(id).into_iter().find(|n| self.has_component(*n, "Transform") && !self.get(*n).is_some_and(|i| i.class.relative_transform))?;
        self.get_pivot(first)
    }

    /// Puts an object's pivot at a world position without moving the object (the Pivot tool):
    /// only `PivotOffset` changes.
    pub fn set_pivot_point(&mut self, id: Id, world: Vec3) -> Result<(), String> {
        let (base, rot) = self.pivot_base(id).ok_or("this object has no pivot")?;
        self.set_prop(id, "PivotOffset", Value::V3(rot.inverse() * (world - base)))
    }
    /// Moves an instance, or everything inside it, keeping the relative layout.
    /// Objects attached to a moved object (relative transforms) follow automatically.
    pub fn pivot_to(&mut self, id: Id, to: (Vec3, Quat)) {
        let Some(from) = self.get_pivot(id) else { return };
        let inv = from.1.inverse();
        for n in self.subtree(id) {
            let relative = self.get(n).is_some_and(|i| i.class.relative_transform);
            if !self.has_component(n, "Transform") || (relative && n != id) {
                continue;
            }
            let Some((p, q)) = self.world_cframe(n) else { continue };
            let new_p = to.0 + to.1 * (inv * (p - from.0));
            let new_q = (to.1 * (inv * q)).normalize();
            if relative {
                // Store the new world pose relative to the parent again.
                let parent_world = self.get(n).and_then(|i| i.parent).filter(|par| self.has_component(*par, "Transform")).and_then(|par| self.world_cframe(par));
                if let Some((pp, pq)) = parent_world {
                    let pqi = pq.inverse();
                    self.set_cframe(n, pqi * (new_p - pp), (pqi * new_q).normalize());
                    continue;
                }
            }
            self.set_cframe(n, new_p, new_q);
        }
    }

    // ---- the primary part of a model ---------------------------------------------------------------

    /// Why `part` cannot be the `PrimaryPart` of `model`, or `None` if it can: it must be a part (not a model, a
    /// light or a script) and inside the model itself, at any depth - never the model, one of its ancestors or
    /// something elsewhere in the scene. The property's validator and the editor's picker both ask this, so what
    /// the picker offers is exactly what the data layer accepts.
    pub fn primary_part_problem(&self, model: Id, part: Id) -> Option<String> {
        if !self.exists(part) {
            return Some("that object no longer exists".into());
        }
        if !self.is_a(part, "BasePart") {
            return Some(format!("PrimaryPart must be a part, but {} is a {}", self.name(part), self.class_name(part)));
        }
        if !self.is_descendant_of(part, model) {
            return Some(format!("PrimaryPart must be a part inside the model: {} is not inside {}", self.name(part), self.name(model)));
        }
        None
    }

    // ---- scaling a model ---------------------------------------------------------------------------

    /// The scale already applied to a model's contents (1 for anything that was never scaled).
    pub fn applied_scale(&self, id: Id) -> f32 {
        self.num(id, "AppliedScale", 1.0)
    }

    /// Scales everything inside a model so that the model as a whole is `scale` times what it was made as.
    ///
    /// Parts have no scale, only a size, so this changes the real data: every part inside (at any depth) gets
    /// its `Size` multiplied and its position moved toward or away from the model's pivot by the same factor,
    /// so rendering, collision, physics and the saved scene all follow. The pivot itself stays put, the parts'
    /// orientations are untouched and the relative layout keeps its shape (the scaling is uniform: a part that
    /// is turned between the model's axes cannot be stretched without shearing). Offsets (`PivotOffset`) and
    /// the local offsets of attached lights scale too; nested models add the factor to their own `AppliedScale`
    /// (their parts are scaled once, by this call, not again by theirs).
    ///
    /// Nothing is changed when the scale is not a positive finite number or when a part would end up outside
    /// the sizes parts may have. Every change goes through `set_prop`, so it is one step of undo.
    pub fn scale_model(&mut self, model: Id, scale: f32) -> Result<(), String> {
        let class = self.get(model).ok_or("instance has been destroyed")?.class;
        if class.prop("AppliedScale").is_none() {
            return Err(format!("{} cannot be scaled", class.name));
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err("Scale must be a positive number".into());
        }
        let before = self.applied_scale(model).max(f32::MIN_POSITIVE);
        let ratio = scale / before;
        if (ratio - 1.0).abs() < 1e-7 {
            return Ok(());
        }
        let origin = self.get_pivot(model).map(|(p, _)| p);
        let inside: Vec<Id> = self.subtree(model).into_iter().skip(1).collect();

        // Check everything first: all of it is scaled or none of it.
        for n in &inside {
            if self.get(*n).is_some_and(|i| i.class.prop("Size").is_some()) {
                let size = self.size(*n) * ratio;
                if size.min_element() < PART_SIZE_RANGE.0 || size.max_element() > PART_SIZE_RANGE.1 {
                    return Err(format!("Scale {scale} would make {} {size} studs: parts can only be {} to {} studs", self.name(*n), PART_SIZE_RANGE.0, PART_SIZE_RANGE.1));
                }
            }
        }
        let Some(origin) = origin.or_else(|| inside.is_empty().then_some(Vec3::ZERO)) else {
            return Err("this model has nothing to scale about (it has no part inside it)".into());
        };

        for n in inside {
            let Some(class) = self.get(n).map(|i| i.class) else { continue };
            if class.has_component("Transform") {
                if let Some((p, q)) = self.cframe(n) {
                    let p = if class.relative_transform { p * ratio } else { origin + (p - origin) * ratio };
                    self.set_cframe(n, p, q);
                }
            }
            if class.prop("Size").is_some() {
                let size = self.size(n) * ratio;
                self.set_prop(n, "Size", Value::V3(size))?;
            }
            if class.prop("PivotOffset").is_some() {
                let offset = self.vec3(n, "PivotOffset") * ratio;
                self.set_prop(n, "PivotOffset", Value::V3(offset))?;
            }
            if class.prop("AppliedScale").is_some() {
                let nested = self.applied_scale(n) * ratio;
                self.set_prop(n, "AppliedScale", Value::Num(nested))?;
            }
        }
        let offset = self.vec3(model, "PivotOffset") * ratio;
        self.set_prop(model, "PivotOffset", Value::V3(offset))?;
        self.set_prop(model, "AppliedScale", Value::Num(scale))
    }

    pub fn translate_parts(&mut self, root: Id, except: Id, delta: Vec3) {
        for n in self.subtree(root) {
            if n == except || !self.has_component(n, "Transform") || self.get(n).is_some_and(|i| i.class.relative_transform) {
                continue;
            }
            if let Some((p, q)) = self.cframe(n) {
                self.set_cframe(n, p + delta, q);
            }
        }
    }

}

#[cfg(test)]
mod pivot_tests {
    use bevy::math::Vec3;

    use crate::{core::Value, dm::DataModel};

    fn part(dm: &mut DataModel, pos: Vec3) -> crate::core::Id {
        let ws = dm.workspace;
        let p = dm.create("Part").unwrap();
        dm.set_prop(p, "Position", Value::V3(pos)).unwrap();
        dm.set_parent(p, Some(ws)).unwrap();
        p
    }

    #[test]
    fn the_pivot_is_the_centre_until_an_offset_is_set_and_the_offset_follows_the_rotation() {
        let mut dm = DataModel::new();
        let p = part(&mut dm, Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(dm.get_pivot(p).unwrap().0, Vec3::new(10.0, 0.0, 0.0));
        dm.set_prop(p, "PivotOffset", Value::V3(Vec3::new(2.0, 0.0, 0.0))).unwrap();
        assert!((dm.get_pivot(p).unwrap().0 - Vec3::new(12.0, 0.0, 0.0)).length() < 1e-5);
        // Turned a quarter around Y, the object's own +X points at -Z, and so does the offset.
        dm.set_prop(p, "Orientation", Value::V3(Vec3::new(0.0, 90.0, 0.0))).unwrap();
        assert!((dm.get_pivot(p).unwrap().0 - Vec3::new(10.0, 0.0, -2.0)).length() < 1e-4);
        assert_eq!(dm.vec3(p, "Position"), Vec3::new(10.0, 0.0, 0.0), "the object itself did not move");
    }

    #[test]
    fn moving_to_a_pivot_puts_the_pivot_there() {
        let mut dm = DataModel::new();
        let p = part(&mut dm, Vec3::new(10.0, 0.0, 0.0));
        dm.set_prop(p, "PivotOffset", Value::V3(Vec3::new(2.0, 0.0, 0.0))).unwrap();
        let (_, q) = dm.get_pivot(p).unwrap();
        dm.pivot_to(p, (Vec3::new(0.0, 5.0, 0.0), q));
        assert!((dm.get_pivot(p).unwrap().0 - Vec3::new(0.0, 5.0, 0.0)).length() < 1e-4, "the pivot is where it was sent");
        assert!((dm.vec3(p, "Position") - Vec3::new(-2.0, 5.0, 0.0)).length() < 1e-4, "so the centre is an offset away from it");
    }

    #[test]
    fn rotating_about_an_offset_pivot_swings_the_object_round_it() {
        let mut dm = DataModel::new();
        let p = part(&mut dm, Vec3::new(10.0, 0.0, 0.0));
        dm.set_prop(p, "PivotOffset", Value::V3(Vec3::new(2.0, 0.0, 0.0))).unwrap();
        let (pivot, q) = dm.get_pivot(p).unwrap();
        let turn = bevy::math::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        dm.pivot_to(p, (pivot, turn * q));
        let (after, _) = dm.get_pivot(p).unwrap();
        assert!((after - pivot).length() < 1e-4, "the pivot stays put while the object turns about it");
        assert!((dm.vec3(p, "Position") - Vec3::new(12.0, 0.0, 2.0)).length() < 1e-4, "the centre swung a quarter turn around it, got {:?}", dm.vec3(p, "Position"));
    }

    #[test]
    fn setting_the_pivot_point_changes_only_the_offset() {
        let mut dm = DataModel::new();
        let p = part(&mut dm, Vec3::new(10.0, 0.0, 0.0));
        dm.set_prop(p, "Orientation", Value::V3(Vec3::new(0.0, 90.0, 0.0))).unwrap();
        dm.set_pivot_point(p, Vec3::new(10.0, 3.0, -4.0)).unwrap();
        assert_eq!(dm.vec3(p, "Position"), Vec3::new(10.0, 0.0, 0.0));
        let offset = dm.vec3(p, "PivotOffset");
        assert!((offset - Vec3::new(4.0, 3.0, 0.0)).length() < 1e-4, "{offset:?}: -4 along world Z is +4 along the turned object's X");
        assert!((dm.get_pivot(p).unwrap().0 - Vec3::new(10.0, 3.0, -4.0)).length() < 1e-4);
    }

    #[test]
    fn a_model_pivots_about_its_primary_part_and_its_own_offset_on_top() {
        let mut dm = DataModel::new();
        let ws = dm.workspace;
        let root = part(&mut dm, Vec3::new(0.0, 0.0, 0.0));
        let other = part(&mut dm, Vec3::new(10.0, 0.0, 0.0));
        let model = dm.create("Model").unwrap();
        dm.set_parent(model, Some(ws)).unwrap();
        dm.set_parent(root, Some(model)).unwrap();
        dm.set_parent(other, Some(model)).unwrap();
        dm.set_prop(model, "PrimaryPart", Value::Ref(Some(root))).unwrap();
        dm.set_prop(root, "PivotOffset", Value::V3(Vec3::new(0.0, 2.0, 0.0))).unwrap();
        assert_eq!(dm.get_pivot(model).unwrap().0, Vec3::new(0.0, 2.0, 0.0), "the primary part's own pivot");
        dm.set_prop(model, "PivotOffset", Value::V3(Vec3::new(1.0, 0.0, 0.0))).unwrap();
        assert_eq!(dm.get_pivot(model).unwrap().0, Vec3::new(1.0, 2.0, 0.0), "plus the model's offset");
        dm.pivot_to(model, (Vec3::new(0.0, 10.0, 0.0), dm.get_pivot(model).unwrap().1));
        assert!((dm.get_pivot(model).unwrap().0 - Vec3::new(0.0, 10.0, 0.0)).length() < 1e-4);
        assert!((dm.vec3(other, "Position") - dm.vec3(root, "Position") - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-4, "the layout is kept");
    }

    #[test]
    fn the_offset_is_saved_with_the_scene() {
        let mut dm = DataModel::new();
        let p = part(&mut dm, Vec3::ZERO);
        dm.set_prop(p, "PivotOffset", Value::V3(Vec3::new(0.0, -1.5, 3.0))).unwrap();
        let place = dm.to_place("x");
        let mut other = DataModel::new();
        other.load_place(&place).unwrap();
        let copy = other.subtree(other.workspace).into_iter().find(|i| other.has_component(*i, "Collider")).unwrap();
        assert_eq!(other.vec3(copy, "PivotOffset"), Vec3::new(0.0, -1.5, 3.0));
    }
}