//! Model: groups world objects so they can be moved together (`PivotTo`, `MoveTo`).

use bevy::math::{Quat, Vec3};

use crate::{core::*, dm::DataModel};

type R = Result<ScriptValue, String>;

fn move_to(c: &mut MethodCall) -> R {
    let pos = c.vec3(0)?;
    if let Some((_, q)) = c.dm.get_pivot(c.this) {
        c.dm.pivot_to(c.this, (pos, q));
    }
    Ok(ScriptValue::Nil)
}

/// The smallest box, turned like the model's pivot, that holds every part inside the model: its centre (in the world)
/// and its size. A model with no part has none.
fn bounding_box(dm: &DataModel, model: Id) -> Option<(Vec3, Quat, Vec3)> {
    let (_, frame) = dm.get_pivot(model)?;
    let inverse = frame.inverse();
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for part in dm.subtree(model).into_iter().filter(|n| dm.is_a(*n, "BasePart")) {
        let Some((p, q)) = dm.world_cframe(part) else { continue };
        let half = dm.size(part) * 0.5;
        for corner in [(-1.0, -1.0, -1.0), (1.0, -1.0, -1.0), (-1.0, 1.0, -1.0), (1.0, 1.0, -1.0), (-1.0, -1.0, 1.0), (1.0, -1.0, 1.0), (-1.0, 1.0, 1.0), (1.0, 1.0, 1.0)] {
            let world = p + q * (half * Vec3::new(corner.0, corner.1, corner.2));
            let local = inverse * world;
            bounds = Some(match bounds {
                Some((lo, hi)) => (lo.min(local), hi.max(local)),
                None => (local, local),
            });
        }
    }
    let (lo, hi) = bounds?;
    Some((frame * ((lo + hi) * 0.5), frame, hi - lo))
}

/// `GetBoundingBox()` -> (CFrame, Vector3): the box that holds the model's parts and its size.
fn get_bounding_box(c: &mut MethodCall) -> R {
    match bounding_box(c.dm, c.this) {
        Some((centre, frame, size)) => Ok(ScriptValue::Multi(vec![ScriptValue::CFrame(centre, frame), ScriptValue::Vec3(size)])),
        None => Ok(ScriptValue::Multi(vec![ScriptValue::CFrame(Vec3::ZERO, Quat::IDENTITY), ScriptValue::Vec3(Vec3::ZERO)])),
    }
}

fn get_extents_size(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Vec3(bounding_box(c.dm, c.this).map_or(Vec3::ZERO, |(_, _, size)| size)))
}

/// `TranslateBy(delta)`: moves the whole model by a vector.
fn translate_by(c: &mut MethodCall) -> R {
    let delta = c.vec3(0)?;
    if let Some((p, q)) = c.dm.get_pivot(c.this) {
        c.dm.pivot_to(c.this, (p + delta, q));
    }
    Ok(ScriptValue::Nil)
}

fn primary_part(c: &MethodCall) -> Result<Id, String> {
    c.dm.reference(c.this, "PrimaryPart").ok_or_else(|| "this model has no PrimaryPart".to_owned())
}

/// `SetPrimaryPartCFrame(cframe)`: moves the model so that its primary part is at the CFrame (the older way to `PivotTo`).
fn set_primary_part_cframe(c: &mut MethodCall) -> R {
    let (p, q) = c.cframe(0)?;
    let part = primary_part(c)?;
    let (pp, pq) = c.dm.world_cframe(part).ok_or("the primary part has no CFrame")?;
    let (mp, mq) = c.dm.get_pivot(c.this).ok_or("this model has no pivot")?;
    // The model keeps its place relative to the primary part: new pivot = target * (primary^-1 * pivot).
    let rel_p = pq.inverse() * (mp - pp);
    let rel_q = pq.inverse() * mq;
    c.dm.pivot_to(c.this, (p + q * rel_p, (q * rel_q).normalize()));
    Ok(ScriptValue::Nil)
}

fn get_primary_part_cframe(c: &mut MethodCall) -> R {
    let part = primary_part(c)?;
    let (p, q) = c.dm.world_cframe(part).ok_or("the primary part has no CFrame")?;
    Ok(ScriptValue::CFrame(p, q))
}

fn scale_to(c: &mut MethodCall) -> R {
    let scale = c.number(0)? as f32;
    c.dm.scale_model(c.this, scale)?;
    Ok(ScriptValue::Nil)
}

fn get_scale_method(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Num(f64::from(c.dm.applied_scale(c.this))))
}

fn nothing(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Nil)
}

// `WorldPivot`: the pivot as a CFrame. Writing it moves the pivot point and leaves the parts where they are (a model's
// pivot keeps the orientation of its primary part or first part: only its position can be moved).
fn get_world_pivot(dm: &DataModel, id: Id) -> Option<Value> {
    dm.get_pivot(id).map(|(p, q)| Value::CF(p, q))
}

fn set_world_pivot(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::CF(p, _) = v else { return Err("WorldPivot must be a CFrame".into()) };
    dm.set_pivot_point(id, p)
}

// A model has no CFrame of its own: its position and rotation are those of its pivot, and
// writing them moves everything inside it (like `PivotTo`). Nothing is saved for them.
fn get_position(dm: &DataModel, id: Id) -> Option<Value> {
    dm.get_pivot(id).map(|(p, _)| Value::V3(p))
}

fn set_position(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::V3(p) = v else { return Err("Position must be a Vector3".into()) };
    let (_, q) = dm.get_pivot(id).ok_or("this model has nothing to move")?;
    dm.pivot_to(id, (p, q));
    Ok(())
}

fn get_orientation(dm: &DataModel, id: Id) -> Option<Value> {
    dm.get_pivot(id).map(|(_, q)| Value::V3(to_euler_deg(q)))
}

fn set_orientation(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::V3(deg) = v else { return Err("Orientation must be a Vector3".into()) };
    let (p, _) = dm.get_pivot(id).ok_or("this model has nothing to turn")?;
    dm.pivot_to(id, (p, euler_deg(deg)));
    Ok(())
}

// The scale of a model's contents. Parts have no scale of their own, only a size, so scaling a model really
// changes what is inside it (see `DataModel::scale_model`); what has been applied so far is kept in
// `AppliedScale`, which is what is saved. `Scale` is the property people and scripts use.
fn get_scale(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Num(dm.applied_scale(id)))
}

fn set_scale(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::Num(scale) = v else { return Err("Scale must be a number".into()) };
    dm.scale_model(id, scale)
}

/// A primary part is a part inside the model itself (at any depth): never the model, an object outside it,
/// or something that is not a part. Unset (`nil`) is always allowed.
fn check_primary_part(dm: &DataModel, model: Id, v: &Value) -> Result<(), String> {
    let Value::Ref(Some(part)) = v else { return Ok(()) };
    match dm.primary_part_problem(model, *part) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

static PROPS: &[PropMeta] = &[
    PropMeta::vector3("Position", 0.0, 0.0, 0.0)
        .group("Transform")
        .order(0)
        .drag(0.1)
        .no_save()
        .desc("Where the model's pivot is. Changing it moves everything inside.")
        .computed(get_position, set_position),
    PropMeta::vector3("Orientation", 0.0, 0.0, 0.0)
        .label("Rotation")
        .group("Transform")
        .order(1)
        .drag(0.5)
        .no_save()
        .desc("Rotation of the model's pivot in degrees (X = pitch, Y = yaw, Z = roll). Changing it turns everything inside.")
        .computed(get_orientation, set_orientation),
    PropMeta::number("Scale", 1.0)
        .group("Transform")
        .order(2)
        .drag(0.01)
        .clamp(MIN_SCALE, MAX_SCALE)
        .no_save()
        .desc("How big the model's contents are, relative to when it was made (1 = as made). Changing it resizes every part inside and moves them toward or away from the model's pivot, so the layout keeps its shape; the pivot stays where it is. Parts added later keep their own size until Scale is changed again.")
        .computed(get_scale, set_scale),
    PropMeta::cframe("WorldPivot").group("Transform").order(3).no_save().hidden().desc("The model's pivot as a CFrame (changing it moves the pivot point, not the parts).").computed(get_world_pivot, set_world_pivot),
    PropMeta::number("AppliedScale", 1.0).group("Transform").hidden().desc("The scale already applied to the contents (saved with the scene; use Scale to change it)."),
    PropMeta::reference("PrimaryPart", "BasePart").group("Model").flags(crate::core::F_PHYS).validate(check_primary_part).desc("The part that defines where the model is (and the body of a character). It must be a part inside the model."),
];

/// The range `Scale` may take.
pub const MIN_SCALE: f32 = 0.001;
pub const MAX_SCALE: f32 = 10_000.0;

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("MoveTo", move_to).desc("Moves the model so its pivot is at the position."),
    MethodDesc::new("GetBoundingBox", get_bounding_box).desc("Returns (CFrame, Vector3): the box that holds the model's parts, turned like its pivot, and its size."),
    MethodDesc::new("GetExtentsSize", get_extents_size).desc("The size of the box that holds the model's parts."),
    MethodDesc::new("TranslateBy", translate_by).desc("Moves the whole model by a vector."),
    MethodDesc::new("SetPrimaryPartCFrame", set_primary_part_cframe).desc("Moves the model so its PrimaryPart is at a CFrame."),
    MethodDesc::new("GetPrimaryPartCFrame", get_primary_part_cframe).desc("The CFrame of the PrimaryPart."),
    MethodDesc::new("ScaleTo", scale_to).desc("Resizes the model's contents so the model is this many times what it was made as."),
    MethodDesc::new("GetScale", get_scale_method).desc("How many times what it was made as the model is now."),
    MethodDesc::new("BreakJoints", nothing).desc("Accepted and ignored here (use BreakJoints on the parts)."),
    MethodDesc::new("MakeJoints", nothing).desc("Accepted and ignored: parts are joined by constraints."),
];

pub static CLASS: ClassDesc = ClassDesc::new("Model", Some("WorldObject"))
    .describe("A group of world objects.")
    .category("Parts")
    .creatable()
    .container()
    .groups(&["Model"])
    .props(PROPS)
    .methods(METHODS);

#[cfg(test)]
mod tests {
    use bevy::math::{Quat, Vec3};

    use crate::{
        core::{Id, Value},
        dm::DataModel,
    };

    fn make(dm: &mut DataModel, class: &str, parent: Id) -> Id {
        let id = dm.create(class).unwrap();
        dm.set_parent(id, Some(parent)).unwrap();
        id
    }

    fn part(dm: &mut DataModel, parent: Id, name: &str, pos: Vec3, size: Vec3) -> Id {
        let p = make(dm, "Part", parent);
        dm.set_name(p, name);
        dm.set_prop(p, "Position", Value::V3(pos)).unwrap();
        dm.set_prop(p, "Size", Value::V3(size)).unwrap();
        dm.set_prop(p, "Anchored", Value::Bool(true)).unwrap();
        p
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-3
    }

    // ---- PrimaryPart ----

    #[test]
    fn a_primary_part_must_be_a_part_inside_the_model() {
        let mut dm = DataModel::new();
        let ws = dm.workspace;
        let outer = make(&mut dm, "Model", ws);
        let inner = make(&mut dm, "Model", outer);
        let in_outer = part(&mut dm, outer, "InOuter", Vec3::ZERO, Vec3::ONE);
        let in_inner = part(&mut dm, inner, "InInner", Vec3::X, Vec3::ONE);
        let elsewhere = part(&mut dm, ws, "Elsewhere", Vec3::Y, Vec3::ONE);
        let set = |dm: &mut DataModel, model: Id, v: Option<Id>| dm.set_prop(model, "PrimaryPart", Value::Ref(v));

        assert!(set(&mut dm, outer, Some(in_outer)).is_ok(), "a part of the model");
        assert!(set(&mut dm, outer, Some(in_inner)).is_ok(), "a part of a model inside it (any depth)");
        assert!(set(&mut dm, inner, Some(in_inner)).is_ok());
        assert!(set(&mut dm, inner, Some(in_outer)).is_err(), "a part of the outer model is not inside the inner one");
        assert!(set(&mut dm, outer, Some(elsewhere)).is_err(), "an unrelated part");
        assert!(set(&mut dm, outer, Some(inner)).is_err(), "a model is not a part");
        assert!(set(&mut dm, outer, Some(outer)).is_err(), "not the model itself");
        assert!(set(&mut dm, inner, Some(outer)).is_err(), "not an ancestor");
        assert_eq!(dm.reference(outer, "PrimaryPart"), Some(in_inner), "a refused write changes nothing");
        assert!(set(&mut dm, outer, None).is_ok(), "and it can be unset");
        assert_eq!(dm.reference(outer, "PrimaryPart"), None);
    }

    #[test]
    fn a_part_that_leaves_or_is_deleted_stops_being_the_primary_part() {
        let mut dm = DataModel::new();
        let ws = dm.workspace;
        let outer = make(&mut dm, "Model", ws);
        let inner = make(&mut dm, "Model", outer);
        let a = part(&mut dm, outer, "A", Vec3::ZERO, Vec3::ONE);
        let b = part(&mut dm, inner, "B", Vec3::X, Vec3::ONE);

        // Moving around inside the model keeps it.
        dm.set_prop(outer, "PrimaryPart", Value::Ref(Some(a))).unwrap();
        dm.set_parent(a, Some(inner)).unwrap();
        assert_eq!(dm.reference(outer, "PrimaryPart"), Some(a), "still inside the outer model");
        // Leaving clears it, and so does moving the model that holds it out of the outer one.
        dm.set_parent(a, Some(ws)).unwrap();
        assert_eq!(dm.reference(outer, "PrimaryPart"), None, "the part left the model");
        assert_eq!(dm.get_prop(outer, "PrimaryPart"), Some(Value::Ref(None)), "cleared, not left pointing at it");
        dm.set_prop(outer, "PrimaryPart", Value::Ref(Some(b))).unwrap();
        dm.set_parent(inner, Some(ws)).unwrap();
        assert_eq!(dm.reference(outer, "PrimaryPart"), None, "its holder left the outer model with it");
        // Deleting it clears it as well.
        let c = part(&mut dm, outer, "C", Vec3::ZERO, Vec3::ONE);
        dm.set_prop(outer, "PrimaryPart", Value::Ref(Some(c))).unwrap();
        dm.destroy(c);
        assert_eq!(dm.get_prop(outer, "PrimaryPart"), Some(Value::Ref(None)));
    }

    // ---- Scale ----

    /// A model with a primary part at the origin, a part 10 studs east, and a nested model with a part far north.
    fn scaled_scene() -> (DataModel, Id, Id, Id, Id, Id) {
        let mut dm = DataModel::new();
        let ws = dm.workspace;
        let model = make(&mut dm, "Model", ws);
        let root = part(&mut dm, model, "Root", Vec3::ZERO, Vec3::new(4.0, 1.0, 2.0));
        let east = part(&mut dm, model, "East", Vec3::new(10.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
        let nested = make(&mut dm, "Model", model);
        let north = part(&mut dm, nested, "North", Vec3::new(0.0, 0.0, 20.0), Vec3::ONE);
        dm.set_prop(model, "PrimaryPart", Value::Ref(Some(root))).unwrap();
        (dm, model, root, east, nested, north)
    }

    #[test]
    fn scaling_a_model_resizes_and_moves_what_is_inside_about_its_pivot() {
        let (mut dm, model, root, east, nested, north) = scaled_scene();
        assert_eq!(dm.get_prop(model, "Scale"), Some(Value::Num(1.0)), "an unscaled model is at 1");
        let pivot = dm.get_pivot(model).unwrap();

        dm.set_prop(model, "Scale", Value::Num(2.0)).unwrap();
        assert_eq!(dm.get_prop(model, "Scale"), Some(Value::Num(2.0)));
        assert_eq!(dm.size(root), Vec3::new(8.0, 2.0, 4.0));
        assert_eq!(dm.size(east), Vec3::splat(4.0));
        assert!(close(dm.vec3(east, "Position"), Vec3::new(20.0, 0.0, 0.0)), "moved away from the pivot by the same factor");
        assert!(close(dm.vec3(north, "Position"), Vec3::new(0.0, 0.0, 40.0)), "also what is in a nested model");
        assert_eq!(dm.size(north), Vec3::splat(2.0));
        assert!(close(dm.get_pivot(model).unwrap().0, pivot.0) && dm.get_pivot(model).unwrap().1.angle_between(pivot.1) < 1e-5, "the pivot did not move");
        assert_eq!(dm.get_prop(nested, "Scale"), Some(Value::Num(2.0)), "the nested model is twice what it was made as, and its parts were scaled only once");

        // The collision volumes and the physics world follow: they are built from the parts' real size and place.
        let (a, b) = dm.collider(east).unwrap().aabb();
        assert!(close(a, Vec3::new(18.0, -2.0, -2.0)) && close(b, Vec3::new(22.0, 2.0, 2.0)), "{a} {b}");

        // Scale is where the model is now, not an amount to add: 2 -> 4 doubles again, and 1 puts everything back.
        dm.set_prop(model, "Scale", Value::Num(4.0)).unwrap();
        assert_eq!(dm.size(east), Vec3::splat(8.0));
        dm.set_prop(model, "Scale", Value::Num(1.0)).unwrap();
        assert_eq!(dm.size(east), Vec3::splat(2.0));
        assert!(close(dm.vec3(east, "Position"), Vec3::new(10.0, 0.0, 0.0)) && close(dm.vec3(north, "Position"), Vec3::new(0.0, 0.0, 20.0)));
        assert_eq!(dm.get_prop(nested, "Scale"), Some(Value::Num(1.0)));
    }

    #[test]
    fn scaling_keeps_the_orientation_of_the_model_and_of_every_part() {
        let (mut dm, model, root, east, _, _) = scaled_scene();
        let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        dm.pivot_to(model, (Vec3::new(5.0, 0.0, 5.0), turn));
        let (pivot, rot_root) = (dm.get_pivot(model).unwrap().0, dm.world_cframe(root).unwrap().1);
        let rot_east = dm.world_cframe(east).unwrap().1;
        let offset = dm.vec3(east, "Position") - pivot;

        dm.set_prop(model, "Scale", Value::Num(3.0)).unwrap();
        assert!(close(dm.get_pivot(model).unwrap().0, pivot));
        assert!(dm.world_cframe(root).unwrap().1.angle_between(rot_root) < 1e-5 && dm.world_cframe(east).unwrap().1.angle_between(rot_east) < 1e-5);
        assert!(close(dm.vec3(east, "Position") - pivot, offset * 3.0), "the layout keeps its shape about the pivot, whichever way the model faces");
    }

    #[test]
    fn a_pivot_offset_and_attached_lights_scale_with_the_model() {
        let (mut dm, model, root, _, _, _) = scaled_scene();
        dm.set_prop(model, "PivotOffset", Value::V3(Vec3::new(1.0, 0.0, 0.0))).unwrap();
        let light = make(&mut dm, "PointLight", root);
        dm.set_prop(light, "Position", Value::V3(Vec3::new(0.0, 3.0, 0.0))).unwrap();
        let pivot = dm.get_pivot(model).unwrap().0;
        let light_before = dm.world_cframe(light).unwrap().0;

        dm.set_prop(model, "Scale", Value::Num(2.0)).unwrap();
        assert!(close(dm.get_pivot(model).unwrap().0, pivot), "with an offset of its own the pivot still stays");
        assert!(close(dm.vec3(model, "PivotOffset"), Vec3::new(2.0, 0.0, 0.0)));
        assert!(close(dm.world_cframe(light).unwrap().0 - pivot, (light_before - pivot) * 2.0), "the light sits where it did on the part, scaled");
    }

    #[test]
    fn a_scale_that_cannot_be_applied_changes_nothing() {
        let (mut dm, model, root, east, _, _) = scaled_scene();
        dm.set_prop(east, "Size", Value::V3(Vec3::new(60_000.0, 1.0, 1.0))).unwrap();
        let before = (dm.size(root), dm.size(east), dm.vec3(east, "Position"));
        assert!(dm.set_prop(model, "Scale", Value::Num(2.0)).is_err(), "a part would be bigger than parts may be");
        assert_eq!((dm.size(root), dm.size(east), dm.vec3(east, "Position")), before, "all or nothing");
        assert_eq!(dm.get_prop(model, "Scale"), Some(Value::Num(1.0)));
        assert!(dm.set_prop(model, "Scale", Value::Num(f32::NAN)).is_err());
        assert!(dm.set_prop(model, "Scale", Value::V3(Vec3::ONE)).is_err(), "only a number");
        // Zero and negative scales are held to the smallest one the property has, like a part's size.
        dm.set_prop(east, "Size", Value::V3(Vec3::splat(2.0))).unwrap();
        dm.set_prop(model, "Scale", Value::Num(-3.0)).unwrap();
        assert_eq!(dm.get_prop(model, "Scale"), Some(Value::Num(super::MIN_SCALE)));
        assert!(dm.size(east).min_element() >= 0.001 - 1e-9, "no part is ever zero or negative");
        // A part cannot be scaled: only a model.
        assert!(dm.scale_model(root, 2.0).is_err());
    }

    #[test]
    fn a_scaled_model_is_saved_as_it_is_and_loads_without_being_scaled_again() {
        let (mut dm, model, _, east, _, north) = scaled_scene();
        dm.set_prop(model, "Scale", Value::Num(2.0)).unwrap();
        let place = dm.to_place("scaled");
        let mut other = DataModel::new();
        other.load_place(&place).unwrap();
        let loaded = other.find_first_child(other.workspace, "East", true).unwrap();
        let loaded_model = other.parent(loaded).unwrap();
        assert_eq!(other.size(loaded), dm.size(east), "the parts are stored at their scaled size, and loading did not scale them again");
        assert!(close(other.vec3(loaded, "Position"), dm.vec3(east, "Position")));
        assert_eq!(other.get_prop(loaded_model, "Scale"), Some(Value::Num(2.0)), "and the model remembers its scale");
        let far = other.find_first_child(other.workspace, "North", true).unwrap();
        assert!(close(other.vec3(far, "Position"), dm.vec3(north, "Position")));
        // The next change continues from the loaded scale.
        other.set_prop(loaded_model, "Scale", Value::Num(1.0)).unwrap();
        assert_eq!(other.size(loaded), Vec3::splat(2.0));
        // A copy of the model is scaled the same, and scaling the copy leaves the original alone.
        let copy = dm.clone_tree(model).unwrap();
        let ws = dm.workspace;
        dm.set_parent(copy, Some(ws)).unwrap();
        assert_eq!(dm.get_prop(copy, "Scale"), Some(Value::Num(2.0)));
        dm.set_prop(copy, "Scale", Value::Num(1.0)).unwrap();
        assert_eq!(dm.size(east), Vec3::splat(4.0), "the original is untouched");
    }
}
