//! Instance: anything that lives in the object tree - it has a Name, a Parent,
//! children, attributes and the tree-navigation methods.

use crate::{core::*, dm::DataModel};

type R = Result<ScriptValue, String>;

fn get_name(dm: &DataModel, id: Id) -> Option<Value> {
    dm.get(id).map(|i| Value::Str(i.name.clone()))
}

fn set_name(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::Str(name) = v else { return Err("Name must be a string".into()) };
    dm.set_name(id, &name);
    Ok(())
}

fn get_parent(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Ref(dm.parent(id)))
}

fn set_parent(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::Ref(parent) = v else { return Err("Parent must be an Instance or nil".into()) };
    dm.set_parent(id, parent)
}

static PROPS: &[PropMeta] = &[
    PropMeta::string("Name", "").group("General").order(0).no_save().desc("What this object is called.").computed(get_name, set_name),
    PropMeta::reference("Parent", "Instance").group("General").order(1).no_save().desc("The object this one is inside. Services cannot be moved.").computed(get_parent, set_parent),
    PropMeta::boolean("Archivable", true).group("General").order(2).hidden().desc("Whether Clone copies this object (and whether it is saved with the scene)."),
];

// ---- methods -------------------------------------------------------------------------------

fn destroy(c: &mut MethodCall) -> R {
    c.dm.destroy(c.this);
    Ok(ScriptValue::Nil)
}

fn clone(c: &mut MethodCall) -> R {
    Ok(ScriptValue::opt_instance(c.dm.clone_tree(c.this)))
}

fn get_children(c: &mut MethodCall) -> R {
    Ok(ScriptValue::instances(c.dm.children(c.this)))
}

fn get_descendants(c: &mut MethodCall) -> R {
    Ok(ScriptValue::instances(c.dm.subtree(c.this).into_iter().skip(1)))
}

fn find_first_child(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    let recursive = c.opt_bool(1).unwrap_or(false);
    Ok(ScriptValue::opt_instance(c.dm.find_first_child(c.this, &name, recursive)))
}

fn find_first_child_of_class(c: &mut MethodCall) -> R {
    let class = c.string(0)?;
    Ok(ScriptValue::opt_instance(c.dm.find_by_class(c.this, &class, false, true)))
}

fn find_first_child_which_is_a(c: &mut MethodCall) -> R {
    let class = c.string(0)?;
    let recursive = c.opt_bool(1).unwrap_or(false);
    Ok(ScriptValue::opt_instance(c.dm.find_by_class(c.this, &class, recursive, false)))
}

fn find_ancestor(c: &MethodCall, test: impl Fn(&DataModel, Id) -> bool) -> Option<Id> {
    let mut cur = c.dm.parent(c.this);
    while let Some(p) = cur {
        if test(c.dm, p) {
            return Some(p);
        }
        cur = c.dm.parent(p);
    }
    None
}

fn find_first_ancestor(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    Ok(ScriptValue::opt_instance(find_ancestor(c, |dm, id| dm.get(id).is_some_and(|i| i.name == name))))
}

fn find_first_ancestor_of_class(c: &mut MethodCall) -> R {
    let class = c.string(0)?;
    Ok(ScriptValue::opt_instance(find_ancestor(c, |dm, id| dm.class_name(id) == class)))
}

fn is_descendant_of(c: &mut MethodCall) -> R {
    let other = c.instance(0)?;
    Ok(ScriptValue::Bool(c.dm.is_descendant_of(c.this, other)))
}

fn is_ancestor_of(c: &mut MethodCall) -> R {
    let other = c.instance(0)?;
    Ok(ScriptValue::Bool(c.dm.is_descendant_of(other, c.this)))
}

fn clear_all_children(c: &mut MethodCall) -> R {
    c.dm.destroy_children(c.this);
    Ok(ScriptValue::Nil)
}

fn get_full_name(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Str(c.dm.full_name(c.this)))
}

fn find_first_ancestor_which_is_a(c: &mut MethodCall) -> R {
    let class = c.string(0)?;
    Ok(ScriptValue::opt_instance(find_ancestor(c, |dm, id| dm.is_a(id, &class))))
}

/// `Remove` (the old name of taking an object out of the tree): the object leaves its parent, and keeps its children.
fn remove(c: &mut MethodCall) -> R {
    c.dm.set_parent(c.this, None)?;
    Ok(ScriptValue::Nil)
}

fn get_debug_id(c: &mut MethodCall) -> R {
    let uid = c.dm.get(c.this).map_or(0, |i| i.uid);
    Ok(ScriptValue::Str(format!("{uid:08X}-0000-4000-8000-{uid:012X}")))
}

// The tag methods of Instance (the same tags `CollectionService` knows).
fn add_tag(c: &mut MethodCall) -> R {
    let tag = c.string(0)?;
    c.dm.add_tag(c.this, &tag);
    Ok(ScriptValue::Nil)
}

fn remove_tag(c: &mut MethodCall) -> R {
    let tag = c.string(0)?;
    c.dm.remove_tag(c.this, &tag);
    Ok(ScriptValue::Nil)
}

fn has_tag(c: &mut MethodCall) -> R {
    let tag = c.string(0)?;
    Ok(ScriptValue::Bool(c.dm.has_tag(c.this, &tag)))
}

fn get_tags(c: &mut MethodCall) -> R {
    Ok(ScriptValue::strings(c.dm.tags_of(c.this)))
}

/// Which actor the object belongs to: this engine has none (Actors only matter for parallel Luau).
fn get_actor(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Nil)
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("Destroy", destroy).desc("Removes this object and everything inside it."),
    MethodDesc::new("Clone", clone).desc("Makes a copy of this object and its descendants (without a parent)."),
    MethodDesc::new("GetChildren", get_children).desc("The direct children."),
    MethodDesc::new("GetDescendants", get_descendants).desc("All descendants."),
    MethodDesc::new("FindFirstChild", find_first_child).desc("The first child with this name (optionally searching deeper)."),
    MethodDesc::new("FindFirstChildOfClass", find_first_child_of_class).desc("The first child of exactly this class."),
    MethodDesc::new("FindFirstChildWhichIsA", find_first_child_which_is_a).desc("The first child that is of or inherits from this class."),
    MethodDesc::new("FindFirstAncestor", find_first_ancestor).desc("The nearest ancestor with this name."),
    MethodDesc::new("FindFirstAncestorOfClass", find_first_ancestor_of_class).desc("The nearest ancestor of exactly this class."),
    MethodDesc::new("IsDescendantOf", is_descendant_of).desc("Whether this object is inside the other one."),
    MethodDesc::new("IsAncestorOf", is_ancestor_of).desc("Whether the other object is inside this one."),
    MethodDesc::new("ClearAllChildren", clear_all_children).desc("Destroys all children."),
    MethodDesc::new("FindFirstAncestorWhichIsA", find_first_ancestor_which_is_a).desc("The nearest ancestor that is of or inherits from this class."),
    MethodDesc::new("GetFullName", get_full_name).desc("The dotted path from the game root."),
    MethodDesc::new("Remove", remove).desc("Takes the object out of its parent (the old name for setting Parent to nil)."),
    MethodDesc::new("GetDebugId", get_debug_id).desc("A string that identifies the object for as long as it exists."),
    MethodDesc::new("AddTag", add_tag).desc("Gives the object a CollectionService tag."),
    MethodDesc::new("RemoveTag", remove_tag).desc("Takes a CollectionService tag off the object."),
    MethodDesc::new("HasTag", has_tag).desc("Whether the object has the tag."),
    MethodDesc::new("GetTags", get_tags).desc("The object's tags."),
    MethodDesc::new("GetActor", get_actor).desc("The Actor the object is in (always nil: there are no Actors)."),
];

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("ChildAdded").desc("A child was added."),
    SignalDesc::new("ChildRemoved").desc("A child was removed."),
    SignalDesc::new("Destroying").desc("The object is about to be destroyed."),
    SignalDesc::new("AncestryChanged").desc("The object or one of its ancestors was moved; passes (the object that moved, its new parent)."),
    SignalDesc::new("DescendantAdded").desc("Something was added anywhere inside this object; passes the new descendant."),
    SignalDesc::new("DescendantRemoving").desc("Something is leaving this object (or being destroyed); passes the descendant."),
    SignalDesc::new("AttributeChanged").desc("An attribute was set, changed or removed; passes its name."),
];

pub static CLASS: ClassDesc = ClassDesc::new("Instance", Some("Object"))
    .describe("An object in the game tree.")
    .groups(&["General"])
    .props(PROPS)
    .methods(METHODS)
    .signals(SIGNALS)
    .luau(include_str!("instance.luau"));
