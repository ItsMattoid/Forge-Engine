//! The instance tree: lookup, parenting and the create / destroy / clone lifecycle.

use std::collections::{HashMap, HashSet};

use super::{journal::Change, DataModel, Event, D_V, D_X};
use crate::core::{class, Id, ResolvedClass, ScriptValue, Value};

impl DataModel {
    // ---- services & lookup ---------------------------------------------------------------

    pub fn service(&self, name: &str) -> Option<Id> {
        self.get(self.game)?.children.iter().copied().find(|c| self.get(*c).is_some_and(|i| i.class.name == name))
    }

    /// All services whose class matches a predicate, in creation order.
    pub fn services_where(&self, pred: impl Fn(&ResolvedClass) -> bool) -> Vec<Id> {
        self.children(self.game).into_iter().filter(|c| self.get(*c).is_some_and(|i| pred(i.class))).collect()
    }

    pub fn parent(&self, id: Id) -> Option<Id> {
        self.get(id)?.parent
    }

    pub fn children(&self, id: Id) -> Vec<Id> {
        self.get(id).map(|i| i.children.clone()).unwrap_or_default()
    }

    pub fn name(&self, id: Id) -> String {
        self.get(id).map(|i| i.name.clone()).unwrap_or_default()
    }

    pub fn class_name(&self, id: Id) -> &'static str {
        self.get(id).map_or("", |i| i.class.name)
    }

    /// Renames an instance. (Scripts rename through the `Name` property, which also fires `Changed`.)
    pub fn set_name(&mut self, id: Id, name: &str) {
        let Some(old) = self.get(id).filter(|i| i.name != name).map(|i| i.name.clone()) else { return };
        self.set_name_raw(id, name);
        self.record(Change::Name { id, old, new: name.to_owned() });
    }

    pub fn find_child(&self, id: Id, name: &str) -> Option<Id> {
        self.get(id)?.children.iter().copied().find(|c| self.get(*c).is_some_and(|i| i.name == name))
    }

    pub fn find_first_child(&self, id: Id, name: &str, recursive: bool) -> Option<Id> {
        if !recursive {
            return self.find_child(id, name);
        }
        self.subtree(id).into_iter().skip(1).find(|c| self.get(*c).is_some_and(|i| i.name == name))
    }

    pub fn find_by_class(&self, id: Id, class_name: &str, recursive: bool, exact: bool) -> Option<Id> {
        let test = |c: &Id| self.get(*c).is_some_and(|i| if exact { i.class.name == class_name } else { i.class.is_a(class_name) });
        if recursive {
            self.subtree(id).into_iter().skip(1).find(test)
        } else {
            self.get(id)?.children.iter().copied().find(test)
        }
    }

    /// `id` and all its descendants, parents before children.
    pub fn subtree(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            let Some(i) = self.get(cur) else { continue };
            out.push(cur);
            stack.extend(i.children.iter().rev().copied());
        }
        out
    }

    pub fn is_descendant_of(&self, id: Id, ancestor: Id) -> bool {
        let mut cur = self.parent(id);
        let mut hops = 0;
        while let Some(p) = cur {
            if p == ancestor {
                return true;
            }
            hops += 1;
            if hops > 10_000 {
                return false;
            }
            cur = self.parent(p);
        }
        false
    }

    pub fn in_workspace(&self, id: Id) -> bool {
        id == self.workspace || self.is_descendant_of(id, self.workspace)
    }

    pub fn full_name(&self, id: Id) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c == self.game {
                break;
            }
            let Some(i) = self.get(c) else { break };
            parts.push(i.name.clone());
            cur = i.parent;
        }
        parts.reverse();
        parts.join(".")
    }

    /// Child indices from the root, used to restore a selection after undo.
    pub fn index_path(&self, id: Id) -> Option<Vec<usize>> {
        let mut path = Vec::new();
        let mut cur = id;
        while cur != self.game {
            let parent = self.parent(cur)?;
            path.push(self.get(parent)?.children.iter().position(|c| *c == cur)?);
            cur = parent;
        }
        path.reverse();
        Some(path)
    }

    pub fn resolve_path(&self, path: &[usize]) -> Option<Id> {
        let mut cur = self.game;
        for &i in path {
            cur = *self.get(cur)?.children.get(i)?;
        }
        Some(cur)
    }

    // ---- creation / destruction ---------------------------------------------------------

    /// Creates an instance of a user-creatable class (what `Instance.new` and the editor use).
    pub fn create(&mut self, class_name: &str) -> Result<Id, String> {
        let c = class(class_name).ok_or_else(|| format!("'{class_name}' is not a valid class name"))?;
        if !c.desc.creatable {
            return Err(format!("cannot create an instance of '{class_name}'"));
        }
        let id = self.spawn_class(c);
        self.record(Change::Created { id, detached: None });
        Ok(id)
    }

    /// Creates what a scene file names: a class scripts can create, or one that is marked `scene_object`.
    pub fn create_for_scene(&mut self, class_name: &str) -> Result<Id, String> {
        match class(class_name) {
            Some(c) if c.desc.scene_object && !c.desc.creatable => {
                let id = self.spawn_class(c);
                self.record(Change::Created { id, detached: None });
                Ok(id)
            }
            _ => self.create(class_name),
        }
    }

    /// Creates an instance of any class (engine-internal: ignores `creatable`).
    pub fn spawn_class(&mut self, c: &'static ResolvedClass) -> Id {
        let id = self.alloc(c);
        c.desc.behavior.on_created(self, id);
        id
    }

    pub fn set_parent(&mut self, id: Id, parent: Option<Id>) -> Result<(), String> {
        let Some(inst) = self.get(id) else { return Err("instance has been destroyed".into()) };
        if inst.class.desc.service_order.is_some() || inst.class.desc.root {
            return Err(format!("{} cannot be re-parented", inst.name));
        }
        let old = inst.parent;
        if let Some(p) = parent {
            if !self.exists(p) {
                return Err("the new parent has been destroyed".into());
            }
            if p == id || self.is_descendant_of(p, id) {
                return Err("an instance cannot be parented to itself or to one of its descendants".into());
            }
        }
        if old == parent {
            return Ok(());
        }
        if let Some(o) = old {
            // (Before the move, so that undoing it puts the part back first and then the reference.)
            self.release_primary_parts(id, o, parent);
        }
        let recording = self.recording();
        let before = if recording { self.slot_in_parent(id) } else { None };
        if let Some(o) = old {
            if let Some(oi) = self.get_mut(o) {
                if let Some(at) = oi.children.iter().position(|c| *c == id) {
                    oi.children.remove(at);
                }
            }
            let ouid = self.get(o).map(|i| i.uid).unwrap_or(0);
            self.emit(ouid, Event::ChildRemoved(o, id));
        }
        if let Some(i) = self.get_mut(id) {
            i.parent = parent;
        }
        if let Some(p) = parent {
            let puid = self.get(p).map(|i| i.uid).unwrap_or(0);
            if let Some(pi) = self.get_mut(p) {
                pi.children.push(id);
            }
            self.emit(puid, Event::ChildAdded(p, id));
        }
        self.reevaluate(id);
        self.announce_move(id, old, parent);
        if let Some(class) = self.get(id).map(|i| i.class) {
            class.desc.behavior.on_parent_changed(self, id, old, parent);
        }
        // The simulation has to find out which of these parts are now in the world and which are not.
        self.phys_mark_subtree(id);
        self.struct_rev += 1;
        if recording {
            let after = self.slot_in_parent(id);
            self.record(Change::Reparent { id, old: before, new: after });
        }
        Ok(())
    }

    /// The instances from `from` up to the root (inclusive) that somebody listens to.
    fn watched_chain(&self, from: Option<Id>) -> Vec<Id> {
        let mut out = Vec::new();
        let mut cur = from;
        let mut hops = 0;
        while let Some(c) = cur {
            let Some(i) = self.get(c) else { break };
            if self.watched.contains(&i.uid) {
                out.push(c);
            }
            hops += 1;
            if hops > 10_000 {
                break;
            }
            cur = i.parent;
        }
        out
    }

    /// Tells scripts that `id` (and everything inside it) moved from under `old` to under `new`:
    /// `AncestryChanged(id, new)` on each of them, `DescendantRemoving` on the old ancestors and
    /// `DescendantAdded` on the new ones. (Nothing is built unless somebody listens.)
    fn announce_move(&mut self, id: Id, old: Option<Id>, new: Option<Id>) {
        if self.watched.is_empty() {
            return;
        }
        let (leaving, joining) = (self.watched_chain(old), self.watched_chain(new));
        let nodes = self.subtree(id);
        let moved = |dm: &mut DataModel, name: &str| {
            let args = || vec![ScriptValue::Instance(id), new.map_or(ScriptValue::Nil, ScriptValue::Instance)];
            for &n in &nodes {
                dm.fire(n, name, args);
            }
        };
        moved(self, "AncestryChanged");
        for ancestor in leaving {
            for &n in &nodes {
                self.fire(ancestor, "DescendantRemoving", || vec![ScriptValue::Instance(n)]);
            }
        }
        for ancestor in joining {
            for &n in &nodes {
                self.fire(ancestor, "DescendantAdded", || vec![ScriptValue::Instance(n)]);
            }
        }
    }

    /// A model's `PrimaryPart` must stay inside the model: when the part (or what it is inside) is about to
    /// leave a model it is the primary part of, the model's reference is cleared first. Only the models above
    /// the object's old place can be affected, so this walks those and nothing else.
    fn release_primary_parts(&mut self, id: Id, old_parent: Id, new_parent: Option<Id>) {
        let mut cur = Some(old_parent);
        while let Some(model) = cur {
            if self.get(model).is_some_and(|i| i.class.prop("PrimaryPart").is_some()) {
                if let Some(primary) = self.reference(model, "PrimaryPart") {
                    let leaving = primary == id || self.is_descendant_of(primary, id);
                    let stays = new_parent.is_some_and(|np| np == model || self.is_descendant_of(np, model));
                    if leaving && !stays {
                        let _ = self.set_prop(model, "PrimaryPart", Value::Ref(None));
                    }
                }
            }
            cur = self.parent(model);
        }
    }

    /// After a subtree moved: renderers must re-check it, scripts may start/stop.
    fn reevaluate(&mut self, id: Id) {
        // A part with nothing inside it (nearly every `Instance.new(...).Parent = ...`) needs no walk.
        if self.get(id).is_some_and(|i| i.children.is_empty()) {
            return self.reevaluate_one(id);
        }
        for n in self.subtree(id) {
            self.reevaluate_one(n);
        }
    }

    fn reevaluate_one(&mut self, n: Id) {
        self.mark(n, D_X | D_V);
        if self.get(n).is_some_and(|i| i.class.has_component("ScriptRunner")) {
            self.pending_scripts.push(n);
        }
    }

    pub fn destroy(&mut self, id: Id) {
        self.destroy_inner(id, true);
    }

    /// Destroys every child of `parent`, as `for c in children do c:Destroy()` would, but without
    /// taking each one out of the parent's child list on its own (that is quadratic in the number of
    /// children). Events come out in the same order.
    pub fn destroy_children(&mut self, parent: Id) {
        // An undo journal records where in the parent each child was, so it needs the list kept current.
        if self.recording() {
            for child in self.children(parent) {
                self.destroy(child);
            }
            return;
        }
        let kids = self.children(parent);
        for &kid in &kids {
            self.destroy_inner(kid, false);
        }
        let gone: HashSet<Id> = kids.into_iter().filter(|k| !self.exists(*k)).collect();
        if let Some(p) = self.get_mut(parent) {
            p.children.retain(|c| !gone.contains(c));
        }
    }

    /// `detach`: whether to take `id` out of its parent's child list (false when the caller does it).
    fn destroy_inner(&mut self, id: Id, detach: bool) {
        let Some(inst) = self.get(id) else { return };
        if inst.class.desc.service_order.is_some() || inst.class.desc.root {
            return;
        }
        let parent = inst.parent;
        if let Some(p) = parent {
            // A model never keeps a primary part that is gone.
            self.release_primary_parts(id, p, None);
        }
        self.record_destroy(id);
        let all = self.subtree(id);
        for &n in &all {
            if let Some(class) = self.get(n).map(|i| i.class) {
                class.desc.behavior.on_destroying(self, n);
            }
            let uid = self.get(n).map(|i| i.uid).unwrap_or(0);
            self.emit(uid, Event::Destroying(n));
            // (The simulation drops what it holds for each of them.)
            self.phys.mark(n);
        }
        self.announce_move(id, parent, None);
        for &n in &all {
            self.drop_tags(n);
        }
        if let Some(p) = parent {
            if let (true, Some(pi)) = (detach, self.get_mut(p)) {
                if let Some(at) = pi.children.iter().position(|c| *c == id) {
                    pi.children.remove(at);
                }
            }
            let puid = self.get(p).map(|i| i.uid).unwrap_or(0);
            self.emit(puid, Event::ChildRemoved(p, id));
        }
        for &n in all.iter().rev() {
            if let Some(slot) = self.slots.get_mut(n.slot as usize) {
                if let Some(inst) = slot.take() {
                    if !self.watched.is_empty() {
                        self.graves.insert(inst.uid, (inst.class.name, inst.name.clone()));
                    }
                    if let Some(e) = inst.entity {
                        self.dead_entities.push(e);
                    }
                    for comp in &inst.class.components {
                        if let Some(set) = self.comp_index.get_mut(comp.name) {
                            set.remove(n.slot);
                        }
                    }
                    if self.watched.remove(&inst.uid) {
                        self.dead_watched.push(inst.uid);
                    }
                    if inst.class.has_component("ScriptRunner") {
                        self.dead_scripts.push(inst.uid);
                    }
                    self.touch_watch.remove(&n);
                    for set in self.tags.values_mut() {
                        set.remove(&n);
                    }
                    self.free.push(n.slot);
                }
            }
        }
        self.struct_rev += 1;
    }

    /// Whether the instance may be copied and saved (its `Archivable` property; true for anything without one).
    pub fn archivable(&self, id: Id) -> bool {
        self.get(id).and_then(|i| i.raw("Archivable")).is_none_or(|v| *v != Value::Bool(false))
    }

    /// `id` and the descendants that may be copied: what is not `Archivable` is left out together with
    /// everything inside it. Parents come before their children.
    fn archivable_subtree(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            let Some(i) = self.get(cur) else { continue };
            if !self.archivable(cur) {
                continue;
            }
            out.push(cur);
            stack.extend(i.children.iter().rev().copied());
        }
        out
    }

    /// Deep copy. The copy has no parent; references inside the copy are remapped. An object that is not
    /// `Archivable` cannot be copied (`None`), and one inside what is copied is left out.
    pub fn clone_tree(&mut self, id: Id) -> Option<Id> {
        let ids = self.archivable_subtree(id);
        if ids.is_empty() || self.get(id)?.class.desc.service_order.is_some() {
            return None;
        }
        let mut map: HashMap<Id, Id> = HashMap::new();
        for &old in &ids {
            let (class, name, props, attrs) = {
                let i = self.get(old)?;
                (i.class, i.name.clone(), i.props.clone(), i.attrs.clone())
            };
            let new = self.alloc(class);
            if let Some(n) = self.get_mut(new) {
                n.name = name;
                n.props = props;
                n.attrs = attrs;
            }
            map.insert(old, new);
        }
        for &old in ids.iter().skip(1) {
            let parent = self.get(old)?.parent?;
            self.link(map[&parent], map[&old]);
        }
        for &new in map.values() {
            if let Some(n) = self.get_mut(new) {
                for (_, v) in n.props.iter_mut() {
                    if let Value::Ref(Some(r)) = v {
                        if let Some(m) = map.get(r) {
                            *v = Value::Ref(Some(*m));
                        }
                    }
                }
            }
        }
        let root = map[&id];
        if let Some(r) = self.get_mut(root) {
            r.parent = None;
        }
        self.record(Change::Created { id: root, detached: None });
        Some(root)
    }
}

#[cfg(test)]
mod tests {
    use crate::dm::{DataModel, Event};

    /// A model with `n` parts in the workspace, all watched so every event is recorded.
    fn workspace_with_parts(n: usize) -> (DataModel, Vec<crate::core::Id>) {
        let mut dm = DataModel::new();
        let ws = dm.workspace;
        let uid = dm.get(ws).unwrap().uid;
        dm.watch(uid);
        let parts: Vec<_> = (0..n)
            .map(|i| {
                let p = dm.create("Part").unwrap();
                dm.set_name(p, &format!("P{i}"));
                dm.set_parent(p, Some(ws)).unwrap();
                let uid = dm.get(p).unwrap().uid;
                dm.watch(uid);
                p
            })
            .collect();
        dm.events.clear();
        (dm, parts)
    }

    #[test]
    fn destroying_all_children_is_the_same_as_destroying_them_one_by_one() {
        let (mut bulk, bulk_parts) = workspace_with_parts(40);
        let (mut each, each_parts) = workspace_with_parts(40);
        bulk.destroy_children(bulk.workspace);
        for c in each.children(each.workspace) {
            each.destroy(c);
        }

        assert!(bulk.children(bulk.workspace).is_empty());
        assert!(bulk_parts.iter().all(|p| !bulk.exists(*p)));
        assert_eq!(bulk.children(bulk.workspace), each.children(each.workspace));
        assert_eq!(bulk.instance_count(), each.instance_count());
        // Same events, in the same order (ids differ between the two models only by name).
        let describe = |dm: &DataModel, parts: &[crate::core::Id]| -> Vec<String> {
            dm.events
                .iter()
                .map(|e| match e {
                    Event::Destroying(id) => format!("destroying {}", parts.iter().position(|p| p == id).map_or("camera".to_owned(), |i| i.to_string())),
                    Event::ChildRemoved(_, id) => format!("removed {}", parts.iter().position(|p| p == id).map_or("camera".to_owned(), |i| i.to_string())),
                    other => format!("{other:?}"),
                })
                .collect()
        };
        assert_eq!(describe(&bulk, &bulk_parts), describe(&each, &each_parts));
        assert_eq!(bulk.dead_entities.len(), each.dead_entities.len());
        assert_eq!(bulk.struct_rev, each.struct_rev);
        assert_eq!(bulk.phys.pending(), each.phys.pending());
    }

    #[test]
    fn destroying_all_children_leaves_what_cannot_be_destroyed_and_works_while_recording() {
        // A service is not destroyed (and stays in the list); with the undo journal on, each child is journaled.
        let mut dm = DataModel::new();
        let game = dm.game;
        let services = dm.children(game);
        dm.destroy_children(game);
        assert_eq!(dm.children(game), services, "services stay");

        let ws = dm.workspace;
        let a = dm.create("Part").unwrap();
        let b = dm.create("Part").unwrap();
        dm.set_parent(a, Some(ws)).unwrap();
        dm.set_parent(b, Some(ws)).unwrap();
        dm.start_journal();
        let before = dm.children(ws).len();
        dm.destroy_children(ws);
        assert!(dm.children(ws).is_empty());
        assert!(dm.take_journal().len() >= before, "every destroyed child was recorded so it can be undone");
    }

    #[test]
    fn reparenting_and_destroying_keep_the_order_of_the_remaining_children() {
        let (mut dm, parts) = workspace_with_parts(6);
        let ws = dm.workspace;
        dm.destroy(parts[2]);
        dm.set_parent(parts[4], None).unwrap();
        dm.set_parent(parts[0], Some(parts[1])).unwrap();
        let names: Vec<String> = dm.children(ws).into_iter().map(|c| dm.name(c)).collect();
        assert_eq!(names, vec!["Camera", "Terrain", "P1", "P3", "P5"]);
        dm.set_parent(parts[4], Some(ws)).unwrap();
        assert_eq!(dm.name(*dm.children(ws).last().unwrap()), "P4", "re-parenting appends");
    }
}
