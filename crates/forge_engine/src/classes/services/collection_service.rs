//! CollectionService: tag objects and find them again by tag.
//!
//! Tags live in the data model (`DataModel::tags`, see `dm::tags`), not in the scripting VM, so they follow
//! an instance for as long as it exists no matter what scripts hold on to. `GetInstanceAddedSignal` /
//! `GetInstanceRemovedSignal` (in `collection_service.luau`) fire when a tag is given or taken (or the instance
//! is destroyed).

use crate::core::*;

type R = Result<ScriptValue, String>;

fn add_tag(c: &mut MethodCall) -> R {
    let inst = c.instance(0)?;
    let tag = c.string(1)?;
    c.dm.add_tag(inst, &tag);
    Ok(ScriptValue::Nil)
}

fn remove_tag(c: &mut MethodCall) -> R {
    let inst = c.instance(0)?;
    let tag = c.string(1)?;
    c.dm.remove_tag(inst, &tag);
    Ok(ScriptValue::Nil)
}

fn has_tag(c: &mut MethodCall) -> R {
    let inst = c.instance(0)?;
    let tag = c.string(1)?;
    Ok(ScriptValue::Bool(c.dm.has_tag(inst, &tag)))
}

fn get_tagged(c: &mut MethodCall) -> R {
    let tag = c.string(0)?;
    Ok(ScriptValue::instances(c.dm.tagged(&tag)))
}

fn get_tags(c: &mut MethodCall) -> R {
    let inst = c.instance(0)?;
    Ok(ScriptValue::strings(c.dm.tags_of(inst)))
}

fn get_all_tags(c: &mut MethodCall) -> R {
    Ok(ScriptValue::strings(c.dm.all_tags()))
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("AddTag", add_tag).desc("Gives an object a tag."),
    MethodDesc::new("RemoveTag", remove_tag).desc("Takes a tag off an object."),
    MethodDesc::new("HasTag", has_tag).desc("Whether the object has the tag."),
    MethodDesc::new("GetTagged", get_tagged).desc("Every object that has the tag."),
    MethodDesc::new("GetTags", get_tags).desc("All tags on an object."),
    MethodDesc::new("GetAllTags", get_all_tags).desc("Every tag that is on at least one object."),
];

pub static CLASS: ClassDesc = ClassDesc::new("CollectionService", Some("Instance"))
    .describe("Tags for objects.")
    .service(12)
    .hidden_in_explorer()
    .methods(METHODS)
    .luau(include_str!("collection_service.luau"));
