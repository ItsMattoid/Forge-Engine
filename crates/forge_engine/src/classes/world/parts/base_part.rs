//! BasePart: a physical, box-shaped thing in the world. Shared by `Part` and
//! `MeshPart`. What it can *do* comes from components:
//!
//! * `Transform`  - CFrame / Position / Orientation
//! * `Appearance` - Color / Material / Transparency / CastShadow
//! * `Collider`   - CanCollide / CanTouch / CanQuery / CollisionGroup and the collision volume
//! * `Rigidbody`  - Anchored, Density, Friction, velocities ... (the physical properties; see `crate::physics`)
//!
//! This file also holds the shared half of the Bevy renderer ([`sync`]): hiding,
//! material, shadows and the entity bookkeeping. A concrete part type supplies the
//! rest as a [`Look`]: where its mesh comes from and how its transform is built.

use bevy::{
    prelude::*,
    camera::primitives::Aabb,
    mesh::MeshTag,
};

use crate::{
    compat,
    core::{unsupported, ClassDesc, EnumDesc, MethodCall, MethodDesc, PropMeta, ScriptValue, SignalDesc, Value, F_PHYS, F_XFORM},
    dm::{DataModel, Id, D_V},
    physics::{
        groups::CollisionGroups,
        joints::{read_joint, JointKind},
    },
    render::{despawn_entity, drawn, entity_of, part_material::pack_tag, set_drawn, set_entity, Drawn, MatKey, Model, RenderCtx},
};

static PROPS: &[PropMeta] = &[
    PropMeta::vector3("Size", 4.0, 1.0, 2.0).group("Transform").order(2).flags(F_XFORM | F_PHYS).drag(0.1).clamp(0.001, 100_000.0).desc("Width, height and depth."),
    PropMeta::boolean("Locked", false).group("Editor").order(0).hidden().desc("Whether the editor refuses to select the part (scripts are not affected)."),
    PropMeta::enumeration("TopSurface", "SurfaceType", "Smooth").group("Appearance").order(10).hidden().desc("What the top of the part is made of (kept for scripts: surfaces are not drawn)."),
    PropMeta::enumeration("BottomSurface", "SurfaceType", "Smooth").group("Appearance").order(11).hidden(),
    PropMeta::enumeration("LeftSurface", "SurfaceType", "Smooth").group("Appearance").order(12).hidden(),
    PropMeta::enumeration("RightSurface", "SurfaceType", "Smooth").group("Appearance").order(13).hidden(),
    PropMeta::enumeration("FrontSurface", "SurfaceType", "Smooth").group("Appearance").order(14).hidden(),
    PropMeta::enumeration("BackSurface", "SurfaceType", "Smooth").group("Appearance").order(15).hidden(),
];

type R = Result<ScriptValue, String>;

// ---- physics methods (see `physics::PhysicsBridge`) --------------------------------------------------

fn apply_impulse(c: &mut MethodCall) -> R {
    let impulse = c.vec3(0)?;
    c.physics.apply_impulse(c.dm, c.this, impulse, None)?;
    Ok(ScriptValue::Nil)
}

fn apply_impulse_at_position(c: &mut MethodCall) -> R {
    let (impulse, at) = (c.vec3(0)?, c.vec3(1)?);
    c.physics.apply_impulse(c.dm, c.this, impulse, Some(at))?;
    Ok(ScriptValue::Nil)
}

fn apply_angular_impulse(c: &mut MethodCall) -> R {
    let impulse = c.vec3(0)?;
    c.physics.apply_angular_impulse(c.dm, c.this, impulse)?;
    Ok(ScriptValue::Nil)
}

fn get_mass(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Num(f64::from(c.physics.mass(c.dm, c.this))))
}

fn get_velocity_at_position(c: &mut MethodCall) -> R {
    let at = c.vec3(0)?;
    Ok(ScriptValue::Vec3(c.physics.velocity_at(c.dm, c.this, at)?))
}

fn wake_up(c: &mut MethodCall) -> R {
    c.physics.set_asleep(c.dm, c.this, false)?;
    Ok(ScriptValue::Nil)
}

fn sleep(c: &mut MethodCall) -> R {
    c.physics.set_asleep(c.dm, c.this, true)?;
    Ok(ScriptValue::Nil)
}

fn get_touching_parts(c: &mut MethodCall) -> R {
    Ok(ScriptValue::instances(c.physics.touching_parts(c.dm, c.this)?))
}

// ---- assemblies and joints ---------------------------------------------------------------------------

/// The active constraints that hold this part to another (or to the world), with the other part: welds, hinges, ropes ...
/// A `NoCollisionConstraint` does not connect parts.
fn joints_of(dm: &DataModel, part: Id) -> Vec<(Id, Option<Id>)> {
    let mut found = Vec::new();
    for joint in dm.with_component("Joint") {
        let Some(spec) = read_joint(dm, joint) else { continue };
        if spec.kind == JointKind::NoCollide {
            continue;
        }
        if spec.a == part {
            found.push((joint, spec.b));
        } else if spec.b == Some(part) {
            found.push((joint, Some(spec.a)));
        }
    }
    found
}

fn get_joints(c: &mut MethodCall) -> R {
    Ok(ScriptValue::instances(joints_of(c.dm, c.this).into_iter().map(|(j, _)| j)))
}

fn break_joints(c: &mut MethodCall) -> R {
    for (joint, _) in joints_of(c.dm, c.this) {
        c.dm.destroy(joint);
    }
    Ok(ScriptValue::Nil)
}

/// `GetConnectedParts(recursive?)`: the parts joined to this one; with `true`, the whole group joined through each other.
fn get_connected_parts(c: &mut MethodCall) -> R {
    let recursive = c.opt_bool(0).unwrap_or(false);
    let mut found: Vec<Id> = Vec::new();
    let mut queue = vec![c.this];
    while let Some(part) = queue.pop() {
        for (_, other) in joints_of(c.dm, part) {
            if let Some(other) = other {
                if other != c.this && !found.contains(&other) {
                    found.push(other);
                    if recursive {
                        queue.push(other);
                    }
                }
            }
        }
    }
    Ok(ScriptValue::instances(found))
}

/// Every part is the root of its own group (welded groups are not tracked as one body).
fn get_root_part(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Instance(c.this))
}

/// `CanCollideWith(part)`: whether the two would collide - both solid, and in collision groups that collide.
fn can_collide_with(c: &mut MethodCall) -> R {
    let other = c.instance(0)?;
    if !c.dm.is_a(other, "BasePart") {
        return Err("argument 1 must be a part".into());
    }
    if !(c.dm.flag(c.this, "CanCollide") && c.dm.flag(other, "CanCollide")) {
        return Ok(ScriptValue::Bool(false));
    }
    let groups = c.dm.service("PhysicsService").map(|s| CollisionGroups::decode(c.dm.str_ref(s, "CollisionGroups")).0).unwrap_or_default();
    let (a, b) = (c.dm.string(c.this, "CollisionGroup"), c.dm.string(other, "CollisionGroup"));
    // (A group that is not registered behaves as Default.)
    let known = |g: String| if groups.contains(&g) { g } else { crate::physics::groups::DEFAULT_GROUP.to_owned() };
    Ok(ScriptValue::Bool(groups.collidable(&known(a), &known(b)).unwrap_or(true)))
}

// Network ownership decides which computer simulates a part. There is one computer.
fn set_network_owner(c: &mut MethodCall) -> R {
    c.opt_instance(0)?;
    Ok(ScriptValue::Nil)
}

fn get_network_owner(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Nil)
}

fn get_network_ownership_auto(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Bool(true))
}

fn can_set_network_ownership(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Multi(vec![ScriptValue::Bool(true), ScriptValue::Nil]))
}

fn nothing(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Nil)
}

/// `Resize(normalId, delta)`: grows or shrinks the part along the axis of a face, keeping the opposite face where it is.
/// Returns whether the new size was allowed.
fn resize(c: &mut MethodCall) -> R {
    let face = c.string(0)?;
    let delta = c.number(1)? as f32;
    let axis = match face.as_str() {
        "Right" => (bevy::math::Vec3::X, 1.0),
        "Left" => (bevy::math::Vec3::X, -1.0),
        "Top" => (bevy::math::Vec3::Y, 1.0),
        "Bottom" => (bevy::math::Vec3::Y, -1.0),
        "Back" => (bevy::math::Vec3::Z, 1.0),
        "Front" => (bevy::math::Vec3::Z, -1.0),
        other => return Err(format!("'{other}' is not a face (Enum.NormalId)")),
    };
    let size = c.dm.size(c.this);
    let new_size = size + axis.0 * delta;
    if !new_size.is_finite() || new_size.min_element() < 0.001 || new_size.max_element() > 100_000.0 {
        return Ok(ScriptValue::Bool(false));
    }
    let (p, q) = c.dm.cframe(c.this).ok_or("this part has no CFrame")?;
    c.dm.set_prop(c.this, "Size", Value::V3(new_size))?;
    c.dm.set_prop(c.this, "CFrame", Value::CF(p + q * (axis.0 * (axis.1 * delta * 0.5)), q))?;
    Ok(ScriptValue::Bool(true))
}

/// `GetClosestPointOnSurface(position)`: the point on the part's surface nearest to a point in the world (the part is
/// treated as its box).
fn closest_point_on_surface(c: &mut MethodCall) -> R {
    let target = c.vec3(0)?;
    let (p, q) = c.dm.world_cframe(c.this).ok_or("this part has no CFrame")?;
    let half = c.dm.size(c.this) * 0.5;
    let local = q.inverse() * (target - p);
    let mut surface = local.clamp(-half, half);
    if surface == local {
        // Inside: the nearest face.
        let gaps = half - local.abs();
        let axis = if gaps.x <= gaps.y && gaps.x <= gaps.z { 0 } else if gaps.y <= gaps.z { 1 } else { 2 };
        surface[axis] = half[axis] * if local[axis] >= 0.0 { 1.0 } else { -1.0 };
    }
    Ok(ScriptValue::Vec3(p + q * surface))
}

fn csg(_c: &mut MethodCall) -> R {
    unsupported("Solid modeling (UnionAsync, SubtractAsync, IntersectAsync)")
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("GetJoints", get_joints).desc("The constraints that hold this part to another part or to the world."),
    MethodDesc::new("BreakJoints", break_joints).desc("Destroys the constraints that hold this part."),
    MethodDesc::new("GetConnectedParts", get_connected_parts).desc("The parts joined to this one; GetConnectedParts(true) also those joined through them."),
    MethodDesc::new("GetRootPart", get_root_part).desc("The part that stands for the group of parts joined to this one (a part is its own group: itself)."),
    MethodDesc::new("CanCollideWith", can_collide_with).desc("Whether this part and another would collide (both solid, in groups that collide)."),
    MethodDesc::new("SetNetworkOwner", set_network_owner).desc("Accepted and ignored: there is one computer."),
    MethodDesc::new("GetNetworkOwner", get_network_owner).desc("Always nil: the one computer simulates everything."),
    MethodDesc::new("GetNetworkOwnershipAuto", get_network_ownership_auto).desc("Always true."),
    MethodDesc::new("SetNetworkOwnershipAuto", nothing).desc("Accepted and ignored: there is one computer."),
    MethodDesc::new("CanSetNetworkOwnership", can_set_network_ownership).desc("Always (true, nil)."),
    MethodDesc::new("Resize", resize).desc("Resize(normalId, delta): grows the part along a face's axis, keeping the opposite face in place. Returns whether it was allowed."),
    MethodDesc::new("GetClosestPointOnSurface", closest_point_on_surface).desc("The point on the part's surface nearest to a point in the world."),
    MethodDesc::new("UnionAsync", csg).desc("Not supported (no solid modeling)."),
    MethodDesc::new("SubtractAsync", csg).desc("Not supported (no solid modeling)."),
    MethodDesc::new("IntersectAsync", csg).desc("Not supported (no solid modeling)."),
    MethodDesc::new("ApplyImpulse", apply_impulse).desc("Gives the part a push at its centre of mass: the impulse (mass * studs per second) changes its velocity at once. Anchored parts cannot be pushed."),
    MethodDesc::new("ApplyImpulseAtPosition", apply_impulse_at_position).desc("Pushes the part at a point in the world (ApplyImpulse(impulse, position)); a push off its centre also makes it turn."),
    MethodDesc::new("ApplyAngularImpulse", apply_angular_impulse).desc("Gives the part a spin: an angular impulse about the world axes."),
    MethodDesc::new("GetMass", get_mass).desc("How much matter the part has (its volume times its Density)."),
    MethodDesc::new("GetVelocityAtPosition", get_velocity_at_position).desc("How fast a point of the part, given in the world, is moving: its velocity plus its turning."),
    MethodDesc::new("WakeUp", wake_up).desc("Wakes the part if the simulation put it to rest."),
    MethodDesc::new("Sleep", sleep).desc("Puts the part to rest until something wakes it."),
    MethodDesc::new("GetTouchingParts", get_touching_parts).desc("The parts this one is touching right now (including parts that do not collide). Both parts need CanTouch."),
];

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("Touched").touch().desc("Something started touching this part; passes the other part."),
    SignalDesc::new("TouchEnded").desc("Something stopped touching this part."),
];

/// Roblox's materials. The renderer draws each as the nearest of its few surface kinds (see `render::material_index`);
/// the physical values a material implies are in `PhysicalProperties.new(Enum.Material.X)`.
pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::numbered(
        "Material",
        &[
            "Plastic", "SmoothPlastic", "Neon", "Wood", "WoodPlanks", "Marble", "Basalt", "Slate", "CrackedLava", "Concrete", "Limestone", "Granite", "Pavement", "Brick", "Pebble", "Cobblestone", "Rock", "Sandstone",
            "CorrodedMetal", "DiamondPlate", "Foil", "Metal", "Grass", "LeafyGrass", "Sand", "Fabric", "Snow", "Mud", "Ground", "Asphalt", "Salt", "Ice", "Glacier", "Glass", "ForceField", "Air", "Water",
        ],
        &[
            256, 272, 288, 512, 528, 784, 788, 800, 804, 816, 820, 832, 836, 848, 864, 880, 896, 912, 1040, 1056, 1072, 1088, 1280, 1284, 1296, 1312, 1328, 1344, 1360, 1376, 1392, 1536, 1552, 1568, 1584, 1792, 2048,
        ],
    ),
];

pub static CLASS: ClassDesc = ClassDesc::new("BasePart", Some("WorldObject"))
    .describe("A solid object in the world.")
    .groups(&["Appearance", "Physics", "Collision", "Rendering", "Editor"])
    .components(&["Transform", "Appearance", "Collider", "Rigidbody"])
    .props(PROPS)
    .methods(METHODS)
    .signals(SIGNALS);

// ---- shared rendering ---------------------------------------------------------------------------

/// The part-type-specific half of rendering.
pub struct Look {
    /// What to draw (a built-in shape, or a model from a file) - see [`Model`].
    pub model: fn(&mut RenderCtx, &DataModel, Id) -> Model,
    /// The Bevy transform for a part at `(position, rotation)` with `size`; `native` is the size the model has before `Size`
    /// scales it (one for the built-in shapes).
    pub transform: fn(&DataModel, Id, Vec3, Quat, Vec3, Vec3) -> Transform,
    /// Texture coordinates are projected along the part's axes in studs (the built-in shapes), not read from the mesh.
    pub studs_uv: bool,
    /// The materials a model file brings are used (unless the part names a material of its own).
    pub imported_materials: bool,
}

/// Position, rotation and a scale equal to the part's size.
pub fn box_transform(_dm: &DataModel, _id: Id, pos: Vec3, rot: Quat, size: Vec3, _native: Vec3) -> Transform {
    Transform { translation: pos, rotation: rot, scale: size }
}

/// Position, rotation and the scale that turns a model of natural size `native` into one of `size`.
pub fn fitted_transform(_dm: &DataModel, _id: Id, pos: Vec3, rot: Quat, size: Vec3, native: Vec3) -> Transform {
    Transform { translation: pos, rotation: rot, scale: size / native.max(Vec3::splat(1e-4)) }
}

/// What one piece of a part is drawn with.
struct PieceLook {
    key: MatKey,
    material: bevy::prelude::Handle<crate::render::PartMaterial>,
    casts_shadow: bool,
}

/// Keeps a part's Bevy entity in step with the instance. Call this from a part
/// type's render hook with its [`Look`].
pub fn sync(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, bits: u8, look: &Look) {
    let transparency = dm.num(id, "Transparency", 0.0);
    let hidden = ctx.opts.mode.hides_transparent() && transparency >= 1.0;
    if !dm.in_workspace(id) || hidden {
        despawn_entity(ctx, dm, id);
        return;
    }
    let Some((pos, rot)) = dm.world_cframe(id) else { return };
    let size = dm.size(id);

    // Only moved or resized: no need to rebuild the mesh and material (and nothing at all when the
    // resulting transform is the one the entity already has).
    let entity = entity_of(dm, id);
    let previous = drawn(dm, id);
    if let (Some(e), false) = (entity, bits & D_V != 0) {
        let transform = (look.transform)(dm, id, pos, rot, size, previous.map_or(Vec3::ONE, |p| p.native));
        match previous {
            Some(p) if p.transform == transform => {}
            Some(p) => {
                set_drawn(dm, id, Drawn { transform, ..p });
                ctx.set_transform(e, transform);
            }
            None => ctx.set_transform(e, transform),
        }
        return;
    }

    // Fully transparent parts stay faintly visible in the editor.
    let alpha = if transparency >= 1.0 { 0.12 } else { 1.0 - transparency };
    let model = (look.model)(ctx, dm, id);
    let transform = (look.transform)(dm, id, pos, rot, size, model.native);

    // The material: the part's own (a `.mat` file by `MaterialVariant`, else the file named like its `Material` item, else the
    // engine's look for the item), or, on a model whose file brought materials, those - unless the part names one.
    let variant = dm.str_ref(id, "MaterialVariant").to_owned();
    let enum_name = dm.str_ref(id, "Material").to_owned();
    let color_map = dm.str_ref(id, "ColorMap").to_owned();
    let model_path = if look.imported_materials { dm.str_ref(id, "MeshId").to_owned() } else { String::new() };
    let use_imported = look.imported_materials && model.has_materials && variant.is_empty() && color_map.is_empty() && dm.flag(id, "UseImportedMaterials");
    let part_see_through = alpha < 1.0;
    let reflectance = dm.num(id, "Reflectance", 0.0);
    let casts = dm.flag(id, "CastShadow") && alpha >= 0.99;

    let mut looks: Vec<PieceLook> = Vec::with_capacity(model.pieces.len());
    let mut part_level: Option<(MatKey, crate::assets::material::MaterialDef)> = None;
    for piece in &model.pieces {
        let (mut key, mut def) = match piece.material.filter(|_| use_imported) {
            Some(i) => match ctx.assets.imported_material(&model_path, i) {
                Some(def) => (ctx.assets.imported_key(&model_path, i, part_see_through, &def), def),
                None => part_level.get_or_insert_with(|| ctx.assets.resolve_material(&variant, &enum_name, &color_map, part_see_through, look.studs_uv, ctx.source)).clone(),
            },
            None => part_level.get_or_insert_with(|| ctx.assets.resolve_material(&variant, &enum_name, &color_map, part_see_through, look.studs_uv, ctx.source)).clone(),
        };
        crate::render::cache::with_reflectance(&mut key, &mut def, reflectance);
        let material = ctx.assets.part_material(&key, &def, ctx.source);
        looks.push(PieceLook { casts_shadow: casts && def.kind.casts_shadow() && def.opacity >= 0.5, key, material });
    }
    let first_mesh = model.pieces[0].mesh.id();
    let ids: Vec<u32> = looks.iter().map(|l| ctx.assets.cache.material_id(&l.key)).collect();
    let multi = looks.len() > 1;
    // The opacity the part itself contributes (the material scales it in the shader).
    let tag = pack_tag(dm.vec3(id, "Color"), alpha);
    let now = Drawn {
        transform,
        mesh: first_mesh,
        native: model.native,
        material: ids.iter().fold(0u32, |acc, i| acc.wrapping_mul(31).wrapping_add(*i)),
        tag,
        casts_shadow: looks[0].casts_shadow,
        multi,
    };

    // A model of several pieces (or one that was, or is no longer): the entity is made again whole. Rare, and the pieces
    // are few, so this is simpler than tracking each child.
    if multi || previous.is_some_and(|p| p.multi) {
        despawn_entity(ctx, dm, id);
        let name = Name::new(dm.name(id));
        let spawned = if multi {
            let parent = ctx.commands.spawn((name, transform, Visibility::default())).id();
            for (piece, look) in model.pieces.iter().zip(&looks) {
                let mut child = ctx.commands.spawn((Mesh3d(piece.mesh.clone()), MeshMaterial3d(look.material.clone()), MeshTag(tag), Transform::default(), ChildOf(parent)));
                if let Some(aabb) = piece.aabb {
                    child.insert(aabb);
                }
                if !look.casts_shadow {
                    child.insert(bevy::light::NotShadowCaster);
                }
            }
            parent
        } else {
            let parts = (name, transform, Mesh3d(model.pieces[0].mesh.clone()), MeshMaterial3d(looks[0].material.clone()), MeshTag(tag));
            let mut e = ctx.commands.spawn(parts);
            if let Some(aabb) = model.pieces[0].aabb {
                e.insert(aabb);
            }
            if !looks[0].casts_shadow {
                e.insert(bevy::light::NotShadowCaster);
            }
            e.id()
        };
        set_entity(dm, id, Some(spawned));
        set_drawn(dm, id, now);
        return;
    }

    let mesh = model.pieces[0].mesh.clone();
    let material = looks.swap_remove(0).material;
    let casts_shadow = now.casts_shadow;
    match entity {
        Some(e) => {
            // Only what differs from what the entity has gets written (everything, if unknown).
            let changed = |same: fn(&Drawn, &Drawn) -> bool| previous.is_none_or(|p| !same(&p, &now));
            if changed(|p, n| p.transform == n.transform) {
                ctx.set_transform(e, transform);
            }
            let mesh_changed = changed(|p, n| p.mesh == n.mesh);
            if mesh_changed {
                // A new mesh needs its own bounding box (Bevy only computes one for entities that have none).
                match model.pieces[0].aabb {
                    Some(aabb) => compat::insert(ctx.commands, e, (Mesh3d(mesh.clone()), aabb)),
                    None => {
                        compat::insert(ctx.commands, e, Mesh3d(mesh.clone()));
                        compat::remove::<Aabb>(ctx.commands, e);
                    }
                }
            }
            // Bevy (with GPU mesh building, the default) re-reads a mesh instance only when one of a fixed list
            // of its components changed: the transform, the visibility, the mesh, the shadow-caster marker being
            // *added* ... and not the `MeshTag` that carries the colour and opacity, nor the marker being
            // removed. So a colour that was edited and nothing else would wait, unseen, for some other change
            // (a move, a resize) to make Bevy look at the instance again. Touching the `Mesh3d` (the same mesh,
            // no new GPU data) puts the instance on that list, and the change shows in the frame it was made.
            let reread = changed(|p, n| p.tag == n.tag) || changed(|p, n| p.casts_shadow == n.casts_shadow) || changed(|p, n| p.material == n.material);
            if reread && !mesh_changed {
                compat::insert(ctx.commands, e, Mesh3d(mesh));
            }
            if changed(|p, n| p.material == n.material) {
                compat::insert(ctx.commands, e, MeshMaterial3d(material));
            }
            if changed(|p, n| p.tag == n.tag) {
                compat::insert(ctx.commands, e, MeshTag(tag));
            }
            if changed(|p, n| p.casts_shadow == n.casts_shadow) {
                if casts_shadow {
                    compat::remove::<bevy::light::NotShadowCaster>(ctx.commands, e);
                } else {
                    compat::insert(ctx.commands, e, bevy::light::NotShadowCaster);
                }
            }
        }
        None => {
            let parts = (Name::new(dm.name(id)), transform, Mesh3d(mesh), MeshMaterial3d(material), MeshTag(tag));
            // One spawn with every component: adding one afterwards (the shadow marker, the bounding
            // box) would move the new entity to another archetype.
            let spawned = match (model.pieces[0].aabb, casts_shadow) {
                (Some(aabb), true) => ctx.commands.spawn((parts, aabb)).id(),
                (Some(aabb), false) => ctx.commands.spawn((parts, aabb, bevy::light::NotShadowCaster)).id(),
                (None, true) => ctx.commands.spawn(parts).id(),
                (None, false) => ctx.commands.spawn((parts, bevy::light::NotShadowCaster)).id(),
            };
            set_entity(dm, id, Some(spawned));
        }
    }
    set_drawn(dm, id, now);
}