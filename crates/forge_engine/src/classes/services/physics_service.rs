//! PhysicsService: collision groups. A part belongs to one group (`CollisionGroup`); the service says which pairs
//! of groups collide. Everything collides with everything until a pair is switched off.
//!
//! The groups are saved with the scene in the `CollisionGroups` property (see `physics::groups` for the format);
//! the methods below read it, change it and write it back, so a change is an ordinary property change: it is
//! undoable in the editor, saved with the place, and noticed by the simulation, which filters every part again.

use crate::{
    core::*,
    dm::DataModel,
    physics::groups::{CollisionGroups, DEFAULT_GROUP},
};

type R = Result<ScriptValue, String>;

static PROPS: &[PropMeta] = &[PropMeta::string("CollisionGroups", "")
    .group("Data")
    .flags(F_PHYS)
    .hidden()
    .multiline()
    .desc("The collision groups and which of them do not collide (edited through the methods of this service).")];

fn read(dm: &DataModel, service: Id) -> CollisionGroups {
    CollisionGroups::decode(dm.str_ref(service, "CollisionGroups")).0
}

fn write(dm: &mut DataModel, service: Id, groups: &CollisionGroups) -> Result<(), String> {
    dm.set_prop(service, "CollisionGroups", Value::Str(groups.encode()))
}

fn register(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    let mut groups = read(c.dm, c.this);
    let index = groups.register(&name)?;
    write(c.dm, c.this, &groups)?;
    Ok(ScriptValue::Num(index as f64))
}

fn unregister(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    let mut groups = read(c.dm, c.this);
    groups.unregister(&name)?;
    // The parts that were in it are back in Default.
    for part in c.dm.with_component("Collider") {
        if c.dm.str_ref(part, "CollisionGroup") == name {
            c.dm.set_prop(part, "CollisionGroup", Value::Str(DEFAULT_GROUP.into()))?;
        }
    }
    write(c.dm, c.this, &groups)?;
    Ok(ScriptValue::Nil)
}

fn set_collidable(c: &mut MethodCall) -> R {
    let (a, b) = (c.string(0)?, c.string(1)?);
    let collide = c.opt_bool(2).ok_or("argument 3 must be a boolean")?;
    let mut groups = read(c.dm, c.this);
    groups.set_collidable(&a, &b, collide)?;
    write(c.dm, c.this, &groups)?;
    Ok(ScriptValue::Nil)
}

fn are_collidable(c: &mut MethodCall) -> R {
    let (a, b) = (c.string(0)?, c.string(1)?);
    Ok(ScriptValue::Bool(read(c.dm, c.this).collidable(&a, &b)?))
}

fn is_registered(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    Ok(ScriptValue::Bool(read(c.dm, c.this).contains(&name)))
}

fn get_registered(c: &mut MethodCall) -> R {
    let groups = read(c.dm, c.this);
    Ok(ScriptValue::List(
        groups.names().iter().enumerate().map(|(i, name)| ScriptValue::Map(vec![("name".into(), ScriptValue::Str(name.clone())), ("id".into(), ScriptValue::Num(i as f64))])).collect(),
    ))
}

fn set_part_group(c: &mut MethodCall) -> R {
    let part = c.instance(0)?;
    let name = c.string(1)?;
    if !read(c.dm, c.this).contains(&name) {
        return Err(format!("'{name}' is not a registered collision group"));
    }
    if !c.dm.is_a(part, "BasePart") {
        return Err("argument 1 must be a part".into());
    }
    c.dm.set_prop(part, "CollisionGroup", Value::Str(name))?;
    Ok(ScriptValue::Nil)
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("RegisterCollisionGroup", register).desc("Adds a collision group (up to 32). It collides with every group until told otherwise. Returns its number."),
    MethodDesc::new("UnregisterCollisionGroup", unregister).desc("Removes a collision group; its parts go back to Default."),
    MethodDesc::new("CollisionGroupSetCollidable", set_collidable).desc("CollisionGroupSetCollidable(groupA, groupB, collidable): whether parts of the two groups collide."),
    MethodDesc::new("CollisionGroupsAreCollidable", are_collidable).desc("Whether parts of the two groups collide."),
    MethodDesc::new("IsCollisionGroupRegistered", is_registered).desc("Whether a group of that name exists."),
    MethodDesc::new("GetRegisteredCollisionGroups", get_registered).desc("The groups, as a list of {name, id}."),
    MethodDesc::new("SetPartCollisionGroup", set_part_group).desc("Puts a part into a (registered) collision group."),
];

pub static CLASS: ClassDesc =
    ClassDesc::new("PhysicsService", Some("Instance")).describe("Collision groups: which parts collide with which.").service(15).hidden_in_explorer().persisted().props(PROPS).methods(METHODS);
