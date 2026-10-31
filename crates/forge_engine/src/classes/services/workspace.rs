//! Workspace: the 3D world. Holds everything that is simulated and drawn, the physics settings of the world
//! (gravity, sleeping, continuous collision) and the spatial queries (`Raycast`, `Spherecast`, `Blockcast`,
//! `GetPartBoundsInBox`, `GetPartBoundsInRadius`, `GetPartsInPart`), which are answered by the physics simulation.

use bevy::math::Vec3;

use crate::{
    core::*,
    dm::{DataModel, D_V, D_X},
    physics::{CastShape, QueryHit, QueryParams},
};

type R = Result<ScriptValue, String>;

/// The `RaycastParams` / `OverlapParams` table a query was given (or the defaults when there was none).
fn query_params(c: &MethodCall, i: usize) -> Result<QueryParams, String> {
    let mut p = QueryParams::default();
    if matches!(c.arg(i), ScriptValue::Nil) {
        return Ok(p);
    }
    if !matches!(c.arg(i), ScriptValue::Map(_)) {
        return Err(format!("argument {} must be a RaycastParams or OverlapParams (or nil)", i + 1));
    }
    if let Some(ScriptValue::List(items)) = c.field(i, "FilterDescendantsInstances") {
        p.filter = items.iter().filter_map(|v| if let ScriptValue::Instance(id) = v { Some(*id) } else { None }).collect();
    }
    if let Some(ScriptValue::Enum(name)) = c.field(i, "FilterType") {
        p.include = name == "Include";
    }
    if let Some(ScriptValue::Str(name)) = c.field(i, "CollisionGroup") {
        p.group = Some(name.clone());
    }
    if let Some(ScriptValue::Bool(b)) = c.field(i, "RespectCanCollide") {
        p.respect_can_collide = *b;
    }
    if let Some(ScriptValue::Num(n)) = c.field(i, "MaxParts") {
        p.max_parts = if n.is_finite() && *n > 0.0 { *n as usize } else { 0 };
    }
    Ok(p)
}

/// A hit as Roblox's `RaycastResult`.
fn hit_to_script(dm: &DataModel, hit: Option<QueryHit>) -> ScriptValue {
    match hit {
        Some(h) => ScriptValue::Typed(
            "RaycastResult",
            Box::new(ScriptValue::Map(vec![
                ("Instance".into(), ScriptValue::Instance(h.id)),
                ("Position".into(), ScriptValue::Vec3(h.position)),
                ("Normal".into(), ScriptValue::Vec3(h.normal)),
                ("Distance".into(), ScriptValue::Num(f64::from(h.distance))),
                ("Material".into(), ScriptValue::EnumItem("Material", dm.string(h.id, "Material"))),
            ])),
        ),
        None => ScriptValue::Nil,
    }
}

/// `workspace:Raycast(origin, direction, raycastParams?)`: the direction is also how far the ray goes.
fn raycast_method(c: &mut MethodCall) -> R {
    let (origin, dir) = (c.vec3(0)?, c.vec3(1)?);
    let params = query_params(c, 2)?;
    let hit = c.physics.raycast(c.dm, origin, dir, &params)?;
    Ok(hit_to_script(c.dm, hit))
}

/// `workspace:Spherecast(position, radius, direction, raycastParams?)`
fn spherecast_method(c: &mut MethodCall) -> R {
    let (origin, radius, dir) = (c.vec3(0)?, c.number(1)? as f32, c.vec3(2)?);
    let params = query_params(c, 3)?;
    let hit = c.physics.shapecast(c.dm, origin, CastShape::Sphere { radius }, dir, &params)?;
    Ok(hit_to_script(c.dm, hit))
}

/// `workspace:Blockcast(cframe, size, direction, raycastParams?)`
fn blockcast_method(c: &mut MethodCall) -> R {
    let ((origin, rotation), size, dir) = (c.cframe(0)?, c.vec3(1)?, c.vec3(2)?);
    let params = query_params(c, 3)?;
    let hit = c.physics.shapecast(c.dm, origin, CastShape::Block { size, rotation }, dir, &params)?;
    Ok(hit_to_script(c.dm, hit))
}

/// `workspace:GetPartBoundsInBox(cframe, size, overlapParams?)`
fn parts_in_box(c: &mut MethodCall) -> R {
    let ((origin, rotation), size) = (c.cframe(0)?, c.vec3(1)?);
    let params = query_params(c, 2)?;
    let found = c.physics.overlap(c.dm, origin, CastShape::Block { size, rotation }, &params)?;
    Ok(ScriptValue::instances(found))
}

/// `workspace:GetPartBoundsInRadius(position, radius, overlapParams?)`
fn parts_in_radius(c: &mut MethodCall) -> R {
    let (origin, radius) = (c.vec3(0)?, c.number(1)? as f32);
    let params = query_params(c, 2)?;
    let found = c.physics.overlap(c.dm, origin, CastShape::Sphere { radius }, &params)?;
    Ok(ScriptValue::instances(found))
}

/// `workspace:GetPartsInPart(part, overlapParams?)`
fn parts_in_part(c: &mut MethodCall) -> R {
    let part = c.instance(0)?;
    let params = query_params(c, 1)?;
    let found = c.physics.overlap_part(c.dm, part, &params)?;
    Ok(ScriptValue::instances(found))
}

static PROPS: &[PropMeta] = &[
    PropMeta::number("Gravity", 196.2).group("World").order(0).flags(F_PHYS).drag(1.0).clamp(0.0, 100_000.0).desc("Downward acceleration in studs per second squared."),
    PropMeta::boolean("AllowSleeping", true).group("World").order(1).flags(F_PHYS).desc("Whether parts that stop moving are put to rest (they cost nothing until something wakes them)."),
    PropMeta::boolean("ContinuousCollision", true)
        .group("World")
        .order(2)
        .flags(F_PHYS)
        .desc("Whether fast parts are checked along their whole path against anchored parts, so they cannot pass through thin walls."),
    PropMeta::number("FallenPartsDestroyHeight", -500.0).group("World").order(3).drag(10.0).clamp(-1_000_000.0, 1_000_000.0).desc("Unanchored parts that fall below this height are destroyed."),
    PropMeta::reference("CurrentCamera", "Camera").runtime().hidden().desc("The camera the game is shown from."),
    PropMeta::number("DistributedGameTime", 0.0).runtime().readonly().hidden().desc("Seconds since the game started (the host writes it every frame)."),
    PropMeta::boolean("StreamingEnabled", false).group("World").order(4).readonly().desc("Roblox streams the world in around the player; this engine always has all of it loaded."),
    PropMeta::boolean("FilteringEnabled", true).group("World").order(5).readonly().hidden().desc("Always true (there is no replication to filter)."),
    PropMeta::boolean("AllowThirdPartySales", false).hidden().desc("Not used (there is no marketplace)."),
];

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("Raycast", raycast_method).desc("Casts a ray (direction is also its length); returns {Instance, Position, Normal, Distance, Material} for the nearest part it hits, or nil."),
    MethodDesc::new("Spherecast", spherecast_method).desc("Sweeps a sphere (position, radius, direction, params?); returns what Raycast returns, or nil."),
    MethodDesc::new("Blockcast", blockcast_method).desc("Sweeps a block (cframe, size, direction, params?); returns what Raycast returns, or nil."),
    MethodDesc::new("GetPartBoundsInBox", parts_in_box).desc("The parts that overlap a box (cframe, size, params?)."),
    MethodDesc::new("GetPartBoundsInRadius", parts_in_radius).desc("The parts that overlap a sphere (position, radius, params?)."),
    MethodDesc::new("GetPartsInPart", parts_in_part).desc("The parts that overlap a part (never the part itself)."),
];

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("RaycastFilterType", &["Exclude", "Include"], &[0, 1]).aliases(&[("Blacklist", "Exclude"), ("Whitelist", "Include")])];

/// Makes sure the world always has a camera to look through.
struct WorkspaceBehavior;

impl ClassBehavior for WorkspaceBehavior {
    /// Switching `CurrentCamera` changes which camera is drawn through: every camera has to
    /// re-run its render hook (the old one gives up its Bevy camera, the new one takes it).
    fn on_prop_changed(&self, dm: &mut DataModel, _id: Id, prop: &PropMeta) {
        if prop.name == "CurrentCamera" {
            for camera in dm.with_component("CameraView") {
                dm.mark(camera, D_X | D_V);
            }
        }
    }

    fn on_loaded(&self, dm: &mut DataModel, id: Id) {
        ensure_camera(dm, id);
        // Every Workspace has a `Terrain` (`workspace.Terrain`); a scene that lacks one gets it.
        if dm.find_by_class(id, "Terrain", false, true).is_none() {
            if let Some(class) = class("Terrain") {
                let terrain = dm.spawn_class(class);
                dm.set_name(terrain, "Terrain");
                let _ = dm.set_parent(terrain, Some(id));
            }
        }
    }
}

/// The workspace's `CurrentCamera`: the one it has, else the first Camera in it, else a new one.
fn ensure_camera(dm: &mut DataModel, id: Id) {
    if dm.reference(id, "CurrentCamera").is_some() {
        return;
    }
    let camera = match dm.find_by_class(id, "Camera", false, true) {
        Some(c) => c,
        None => {
            let Some(class) = class("Camera") else { return };
            let c = dm.spawn_class(class);
            dm.set_name(c, "Camera");
            let _ = dm.set_prop(c, "Position", Value::V3(Vec3::new(0.0, 10.0, 20.0)));
            let _ = dm.set_parent(c, Some(id));
            c
        }
    };
    let _ = dm.set_prop(id, "CurrentCamera", Value::Ref(Some(camera)));
}

pub static CLASS: ClassDesc = ClassDesc::new("Workspace", Some("Instance"))
    .describe("The 3D world.")
    .service(0)
    .persisted()
    .runs_scripts()
    .container()
    .world_root()
    .groups(&["World"])
    .props(PROPS)
    .methods(METHODS)
    .luau(include_str!("workspace.luau"))
    .behavior(&WorkspaceBehavior);
