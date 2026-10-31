//! The change journal: what command-based undo is built on.
//!
//! While journaling is on, every low-level edit of the data model (a property write, a rename,
//! a re-parent, creating or destroying an instance) leaves a reversible [`Change`] behind. The
//! editor takes the changes at the end of each user action and keeps them as one undoable command.
//! Undoing replays the changes backwards, redoing replays them forwards, and nothing else about
//! the scene is touched (unlike a snapshot restore, which rebuilds every instance).
//!
//! Instances keep their identity as far as possible. The one case where that is impossible is
//! bringing back a destroyed instance: an [`Id`] is never reused, so the revived instance gets a
//! new one. [`IdMap`] remembers `old id -> new id` and every later replay looks ids up through it,
//! so older commands that mention the instance still reach it.
//!
//! Journaling is off by default (a running game records nothing).

use std::collections::{HashMap, HashSet};

use super::{file::InstanceDef, DataModel};
use crate::core::{Id, PropType, Value};

// ---- ids --------------------------------------------------------------------------------------

/// `id recorded in a change -> the instance that stands for it now`.
#[derive(Default, Debug, Clone)]
pub struct IdMap {
    map: HashMap<Id, Id>,
}

impl IdMap {
    /// Follows the chain of replacements (an instance can be revived more than once).
    pub fn resolve(&self, id: Id) -> Id {
        let mut cur = id;
        for _ in 0..10_000 {
            match self.map.get(&cur) {
                Some(next) if *next != cur => cur = *next,
                _ => break,
            }
        }
        cur
    }

    /// `from` is now represented by `to`.
    pub fn link(&mut self, from: Id, to: Id) {
        if from != to {
            self.map.insert(from, to);
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    fn value(&self, v: &Value) -> Value {
        match v {
            Value::Ref(Some(id)) => Value::Ref(Some(self.resolve(*id))),
            other => other.clone(),
        }
    }
}

// ---- changes ----------------------------------------------------------------------------------

/// A subtree taken out of the data model (or about to be), with what is needed to rebuild it.
#[derive(Debug, Clone)]
pub struct Detached {
    pub def: InstanceDef,
    /// The instances of the subtree, depth-first (what `def` was written from).
    pub ids: Vec<Id>,
    /// References from inside the subtree to instances outside it: `(index into ids, property, target)`.
    pub external: Vec<(usize, &'static str, Id)>,
}

/// One reversible edit.
#[derive(Debug, Clone)]
pub enum Change {
    Prop { id: Id, name: &'static str, old: Value, new: Value },
    Name { id: Id, old: String, new: String },
    /// `(parent, position among its children)` before and after; `None` is "no parent".
    Reparent { id: Id, old: Option<(Id, usize)>, new: Option<(Id, usize)> },
    /// A new instance (with whatever it holds) came into being, without a parent.
    /// `detached` is filled in when the creation is undone, so it can be redone.
    Created { id: Id, detached: Option<Box<Detached>> },
    /// A subtree was destroyed. `inbound` are references from elsewhere that pointed into it.
    Destroyed { detached: Box<Detached>, parent: Option<(Id, usize)>, inbound: Vec<(Id, &'static str, Id)> },
}

impl Change {
    /// The instance the change is mainly about (what the editor selects after undoing it).
    pub fn focus(&self) -> Option<Id> {
        match self {
            Change::Prop { id, .. } | Change::Name { id, .. } | Change::Reparent { id, .. } | Change::Created { id, .. } => Some(*id),
            Change::Destroyed { detached, .. } => detached.ids.first().copied(),
        }
    }
}

#[derive(Default)]
pub struct Journal {
    changes: Vec<Change>,
    /// Where the latest `Prop` change of each property is, so a drag that writes the same
    /// value a hundred times is one change.
    prop_at: HashMap<(Id, &'static str), usize>,
}

impl Journal {
    fn push(&mut self, change: Change) {
        match change {
            Change::Prop { id, name, old, new } => {
                if let Some(&i) = self.prop_at.get(&(id, name)) {
                    if let Change::Prop { new: latest, .. } = &mut self.changes[i] {
                        *latest = new;
                        return;
                    }
                }
                self.prop_at.insert((id, name), self.changes.len());
                self.changes.push(Change::Prop { id, name, old, new });
            }
            other => {
                // Anything else may change what an earlier property write meant: stop merging across it.
                self.prop_at.clear();
                self.changes.push(other);
            }
        }
    }
}

// ---- recording ---------------------------------------------------------------------------------

impl DataModel {
    /// Starts recording edits (and forgets anything recorded before).
    pub fn start_journal(&mut self) {
        self.journal = Some(Journal::default());
    }

    pub fn stop_journal(&mut self) {
        self.journal = None;
    }

    pub fn journaling(&self) -> bool {
        self.journal.is_some()
    }

    /// Edits have been recorded that nobody has taken yet.
    pub fn journal_pending(&self) -> bool {
        self.journal.as_ref().is_some_and(|j| !j.changes.is_empty())
    }

    /// The edits recorded since the last call, oldest first. Property writes that ended where
    /// they started (a drag that came back to its origin) are left out.
    pub fn take_journal(&mut self) -> Vec<Change> {
        let Some(j) = self.journal.as_mut() else { return Vec::new() };
        j.prop_at.clear();
        std::mem::take(&mut j.changes).into_iter().filter(|c| !matches!(c, Change::Prop { old, new, .. } if old == new)).collect()
    }

    pub(super) fn recording(&self) -> bool {
        self.journal.is_some() && !self.journal_paused
    }

    pub(super) fn record(&mut self, change: Change) {
        if self.journal_paused {
            return;
        }
        if let Some(j) = self.journal.as_mut() {
            j.push(change);
        }
    }

    // ---- capturing subtrees ---------------------------------------------------------------------

    /// Writes a subtree down so it can be rebuilt later.
    pub fn capture_subtree(&self, root: Id) -> Detached {
        let ids = self.subtree(root);
        let index: HashMap<Id, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i + 1)).collect();
        let def = self.encode(root, &index);
        let mut external = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            let Some(inst) = self.get(*id) else { continue };
            for rp in inst.class.props.iter().filter(|rp| matches!(rp.meta.ty, PropType::Ref(_)) && rp.meta.saved() && rp.meta.getter.is_none()) {
                if let Some(Value::Ref(Some(target))) = inst.raw(rp.meta.name) {
                    if !index.contains_key(target) && self.exists(*target) {
                        external.push((i, rp.meta.name, *target));
                    }
                }
            }
        }
        Detached { def, ids, external }
    }

    /// References held by instances outside `ids` that point at one of them.
    fn inbound_refs(&self, ids: &[Id]) -> Vec<(Id, &'static str, Id)> {
        let inside: HashSet<Id> = ids.iter().copied().collect();
        let mut out = Vec::new();
        for holder in self.all_ids() {
            if inside.contains(&holder) {
                continue;
            }
            let Some(inst) = self.get(holder) else { continue };
            for rp in inst.class.props.iter().filter(|rp| matches!(rp.meta.ty, PropType::Ref(_)) && rp.meta.getter.is_none()) {
                if let Some(Value::Ref(Some(target))) = inst.raw(rp.meta.name) {
                    if inside.contains(target) {
                        out.push((holder, rp.meta.name, *target));
                    }
                }
            }
        }
        out
    }

    /// Called by `destroy` before it removes `id`.
    pub(super) fn record_destroy(&mut self, id: Id) {
        if !self.recording() {
            return;
        }
        let detached = self.capture_subtree(id);
        let inbound = self.inbound_refs(&detached.ids);
        let parent = self.slot_in_parent(id);
        self.record(Change::Destroyed { detached: Box::new(detached), parent, inbound });
    }

    /// `(parent, position among its children)`.
    pub fn slot_in_parent(&self, id: Id) -> Option<(Id, usize)> {
        let parent = self.get(id)?.parent?;
        let pos = self.get(parent)?.children.iter().position(|c| *c == id)?;
        Some((parent, pos))
    }

    /// Moves `id` under `parent` at a given position among its children (the end, if the position
    /// is past it). Recorded as one re-parent, so it undoes as one step.
    pub fn set_parent_at(&mut self, id: Id, parent: Id, index: usize) -> Result<(), String> {
        let recording = self.recording();
        let before = self.slot_in_parent(id);
        self.without_journal(|dm| dm.place(id, Some((parent, index))))?;
        if recording {
            let after = self.slot_in_parent(id);
            if after != before {
                self.record(Change::Reparent { id, old: before, new: after });
            }
        }
        Ok(())
    }

    /// Creates a copy of a captured subtree (what Paste does) and puts it under `parent`.
    /// Recorded as a creation followed by a re-parent.
    pub fn paste_subtree(&mut self, d: &Detached, parent: Id) -> Result<Id, String> {
        let root = self.without_journal(|dm| dm.restore_subtree(d, &mut IdMap::default()))?;
        self.record(Change::Created { id: root, detached: None });
        self.set_parent(root, Some(parent))?;
        Ok(root)
    }

    // ---- replaying ------------------------------------------------------------------------------

    /// Runs `f` with recording paused (so what it does is not itself recorded).
    pub fn without_journal<R>(&mut self, f: impl FnOnce(&mut DataModel) -> R) -> R {
        let was = std::mem::replace(&mut self.journal_paused, true);
        let r = f(self);
        self.journal_paused = was;
        r
    }

    /// Reverses `change`.
    pub fn undo_change(&mut self, change: &mut Change, ids: &mut IdMap) -> Result<(), String> {
        self.without_journal(|dm| dm.replay(change, ids, true))
    }

    /// Applies `change` again after it was undone.
    pub fn redo_change(&mut self, change: &mut Change, ids: &mut IdMap) -> Result<(), String> {
        self.without_journal(|dm| dm.replay(change, ids, false))
    }

    fn replay(&mut self, change: &mut Change, ids: &mut IdMap, undo: bool) -> Result<(), String> {
        match change {
            Change::Prop { id, name, old, new } => {
                let target = ids.resolve(*id);
                let value = ids.value(if undo { old } else { new });
                self.set_prop(target, name, value)
            }
            Change::Name { id, old, new } => {
                let target = ids.resolve(*id);
                if !self.exists(target) {
                    return Err("the renamed object no longer exists".into());
                }
                self.set_name(target, if undo { old } else { new });
                Ok(())
            }
            Change::Reparent { id, old, new } => {
                let target = ids.resolve(*id);
                let slot = if undo { *old } else { *new };
                self.place(target, slot.map(|(p, i)| (ids.resolve(p), i)))
            }
            Change::Created { id, detached } => {
                if undo {
                    let root = ids.resolve(*id);
                    if !self.exists(root) {
                        return Err("the created object no longer exists".into());
                    }
                    *detached = Some(Box::new(self.capture_subtree(root)));
                    self.destroy(root);
                    Ok(())
                } else {
                    let d = detached.as_ref().ok_or("nothing to recreate")?;
                    self.restore_subtree(d, ids).map(|_| ())
                }
            }
            Change::Destroyed { detached, parent, inbound } => {
                if undo {
                    let root = self.restore_subtree(detached, ids)?;
                    let placed = self.place(root, parent.map(|(p, i)| (ids.resolve(p), i)));
                    for (holder, name, target) in inbound.iter() {
                        let (holder, target) = (ids.resolve(*holder), ids.resolve(*target));
                        if self.exists(holder) && self.exists(target) {
                            let _ = self.set_prop(holder, name, Value::Ref(Some(target)));
                        }
                    }
                    placed
                } else {
                    let root = detached.ids.first().map(|id| ids.resolve(*id)).ok_or("nothing to destroy")?;
                    if !self.exists(root) {
                        return Err("the object to remove no longer exists".into());
                    }
                    self.destroy(root);
                    Ok(())
                }
            }
        }
    }

    /// Puts `id` under `slot.0` at position `slot.1` (or takes it out of the tree for `None`).
    fn place(&mut self, id: Id, slot: Option<(Id, usize)>) -> Result<(), String> {
        self.set_parent(id, slot.map(|(p, _)| p))?;
        if let Some((parent, index)) = slot {
            if let Some(p) = self.get_mut(parent) {
                if let Some(at) = p.children.iter().position(|c| *c == id) {
                    p.children.remove(at);
                    p.children.insert(index.min(p.children.len()), id);
                }
            }
            self.mark_dependents(parent);
        }
        Ok(())
    }

    /// Rebuilds a captured subtree (detached) and teaches `ids` which new instance stands for which old one.
    fn restore_subtree(&mut self, d: &Detached, ids: &mut IdMap) -> Result<Id, String> {
        let mut created = Vec::new();
        let mut refs = Vec::new();
        let mut warns = Vec::new();
        let root = self.decode(&d.def, None, &mut created, &mut refs, &mut warns).ok_or_else(|| format!("could not rebuild the object: {}", warns.join("; ")))?;
        self.resolve_refs(refs, &created, &mut warns);
        for (old, new) in d.ids.iter().zip(&created) {
            if let Some(new) = new {
                ids.link(*old, *new);
            }
        }
        for (i, name, target) in &d.external {
            let Some(Some(holder)) = created.get(*i) else { continue };
            let target = ids.resolve(*target);
            if self.exists(target) {
                let _ = self.set_prop(*holder, name, Value::Ref(Some(target)));
            }
        }
        Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec3;

    use super::*;

    /// What the editor's history does, minimal: commands made of changes, one id map.
    #[derive(Default)]
    struct Stack {
        done: Vec<Vec<Change>>,
        undone: Vec<Vec<Change>>,
        ids: IdMap,
    }

    impl Stack {
        /// Ends the current user action: everything recorded since the last call is one command.
        fn commit(&mut self, dm: &mut DataModel) {
            let changes = dm.take_journal();
            if !changes.is_empty() {
                self.done.push(changes);
                self.undone.clear();
            }
        }

        fn undo(&mut self, dm: &mut DataModel) {
            let mut cmd = self.done.pop().expect("something to undo");
            for c in cmd.iter_mut().rev() {
                dm.undo_change(c, &mut self.ids).expect("undo step");
            }
            self.undone.push(cmd);
        }

        fn redo(&mut self, dm: &mut DataModel) {
            let mut cmd = self.undone.pop().expect("something to redo");
            for c in cmd.iter_mut() {
                dm.redo_change(c, &mut self.ids).expect("redo step");
            }
            self.done.push(cmd);
        }
    }

    fn world() -> (DataModel, Stack) {
        let mut dm = DataModel::new();
        dm.start_journal();
        (dm, Stack::default())
    }

    fn add(dm: &mut DataModel, class: &str, name: &str, parent: Id) -> Id {
        let id = dm.create(class).unwrap();
        dm.set_name(id, name);
        dm.set_parent(id, Some(parent)).unwrap();
        id
    }

    fn named(dm: &DataModel, parent: Id, name: &str) -> Id {
        dm.find_child(parent, name).unwrap_or_else(|| panic!("no child named {name}"))
    }

    #[test]
    fn a_property_edit_undoes_and_redoes_and_a_drag_is_one_change() {
        let (mut dm, mut h) = world();
        let ws = dm.workspace;
        let part = add(&mut dm, "Part", "P", ws);
        h.commit(&mut dm);

        for i in 1..=50 {
            dm.set_prop(part, "Position", Value::V3(Vec3::new(i as f32, 0.0, 0.0))).unwrap();
        }
        let drag = dm.take_journal();
        assert_eq!(drag.len(), 1, "fifty writes of one property are one change");
        h.done.push(drag);

        h.undo(&mut dm);
        assert_eq!(dm.vec3(part, "Position"), Vec3::ZERO);
        h.redo(&mut dm);
        assert_eq!(dm.vec3(part, "Position"), Vec3::new(50.0, 0.0, 0.0));
        assert!(dm.exists(part), "editing never replaces the instance");
    }

    #[test]
    fn a_drag_that_ends_where_it_started_leaves_nothing_to_undo() {
        let (mut dm, _) = world();
        let ws = dm.workspace;
        let part = add(&mut dm, "Part", "P", ws);
        dm.take_journal();
        dm.set_prop(part, "Position", Value::V3(Vec3::X)).unwrap();
        dm.set_prop(part, "Position", Value::V3(Vec3::ZERO)).unwrap();
        assert!(dm.take_journal().is_empty());
    }

    #[test]
    fn inserting_and_deleting_round_trip_with_children_references_and_position() {
        let (mut dm, mut h) = world();
        let ws = dm.workspace;
        let first = add(&mut dm, "Folder", "First", ws);
        let model = add(&mut dm, "Model", "M", ws);
        let last = add(&mut dm, "Folder", "Last", ws);
        let root = add(&mut dm, "Part", "Root", model);
        let tail = add(&mut dm, "Part", "Tail", model);
        dm.set_prop(model, "PrimaryPart", Value::Ref(Some(root))).unwrap();
        let pointer = add(&mut dm, "ObjectValue", "Pointer", ws);
        dm.set_prop(pointer, "Value", Value::Ref(Some(root))).unwrap();
        dm.set_prop(tail, "Size", Value::V3(Vec3::new(7.0, 7.0, 7.0))).unwrap();
        h.commit(&mut dm);
        let order = |dm: &DataModel| dm.children(ws).iter().map(|c| dm.name(*c)).collect::<Vec<_>>();
        let before = order(&dm);
        assert!(before.iter().position(|n| n == "M") < before.iter().position(|n| n == "Last"));

        // Delete the whole model.
        dm.destroy(model);
        h.commit(&mut dm);
        assert!(!dm.exists(model) && !dm.exists(root));
        assert!(dm.reference(pointer, "Value").is_none(), "the pointer is dangling while the model is gone");

        h.undo(&mut dm);
        assert_eq!(order(&dm), before, "the model is back between its old neighbours");
        let model = named(&dm, ws, "M");
        let (root, tail) = (named(&dm, model, "Root"), named(&dm, model, "Tail"));
        assert_eq!(dm.size(tail), Vec3::new(7.0, 7.0, 7.0), "its contents came back too");
        assert_eq!(dm.reference(model, "PrimaryPart"), Some(root), "references inside it are rebuilt");
        assert_eq!(dm.reference(pointer, "Value"), Some(root), "references from outside point at the revived part");

        h.redo(&mut dm);
        assert!(dm.find_child(ws, "M").is_none());
        h.undo(&mut dm);
        assert!(dm.find_child(ws, "M").is_some(), "and it can come back again");
        let _ = (first, last);
    }

    #[test]
    fn older_commands_still_reach_an_instance_that_was_deleted_and_revived() {
        let (mut dm, mut h) = world();
        let ws = dm.workspace;
        let part = add(&mut dm, "Part", "P", ws);
        h.commit(&mut dm);
        dm.set_prop(part, "Transparency", Value::Num(0.5)).unwrap();
        h.commit(&mut dm); // 1: transparency
        dm.destroy(part);
        h.commit(&mut dm); // 2: delete

        h.undo(&mut dm); // revive (new id)
        let revived = named(&dm, ws, "P");
        assert_ne!(revived, part, "ids are never reused");
        assert_eq!(dm.num(revived, "Transparency", -1.0), 0.5);

        h.undo(&mut dm); // undo the transparency edit on the revived instance
        assert_eq!(dm.num(revived, "Transparency", -1.0), 0.0);
        h.redo(&mut dm);
        h.redo(&mut dm); // the delete again
        assert!(dm.find_child(ws, "P").is_none());
        h.undo(&mut dm);
        h.undo(&mut dm);
        h.undo(&mut dm); // all the way back: creating the part is undone
        assert!(dm.find_child(ws, "P").is_none());
        h.redo(&mut dm); // create
        h.redo(&mut dm); // transparency
        assert_eq!(dm.num(named(&dm, ws, "P"), "Transparency", -1.0), 0.5, "the edit reaches the re-created part");
        h.redo(&mut dm); // delete
        assert!(dm.find_child(ws, "P").is_none(), "after the final redo the part is deleted");
    }

    #[test]
    fn creating_and_cloning_undo_to_nothing_and_redo_to_the_same_content() {
        let (mut dm, mut h) = world();
        let ws = dm.workspace;
        let model = add(&mut dm, "Model", "M", ws);
        add(&mut dm, "Part", "Inside", model);
        h.commit(&mut dm);

        let copy = dm.clone_tree(model).unwrap();
        dm.set_parent(copy, Some(ws)).unwrap();
        h.commit(&mut dm);
        assert_eq!(dm.children(ws).iter().filter(|c| dm.name(**c) == "M").count(), 2);

        h.undo(&mut dm);
        assert_eq!(dm.children(ws).iter().filter(|c| dm.name(**c) == "M").count(), 1);
        h.redo(&mut dm);
        let copies: Vec<Id> = dm.children(ws).into_iter().filter(|c| dm.name(*c) == "M").collect();
        assert_eq!(copies.len(), 2);
        assert!(copies.iter().all(|m| dm.find_child(*m, "Inside").is_some()), "the clone came back with its child");
    }

    #[test]
    fn reparenting_and_renaming_undo() {
        let (mut dm, mut h) = world();
        let ws = dm.workspace;
        let a = add(&mut dm, "Folder", "A", ws);
        let b = add(&mut dm, "Folder", "B", ws);
        let x = add(&mut dm, "Part", "X", a);
        let y = add(&mut dm, "Part", "Y", a);
        h.commit(&mut dm);

        dm.set_parent(x, Some(b)).unwrap();
        h.commit(&mut dm);
        dm.set_name(y, "Renamed");
        h.commit(&mut dm);

        h.undo(&mut dm);
        assert_eq!(dm.name(y), "Y");
        h.undo(&mut dm);
        assert_eq!(dm.parent(x), Some(a));
        assert_eq!(dm.children(a), vec![x, y], "X is back in front of Y");
        h.redo(&mut dm);
        assert_eq!(dm.parent(x), Some(b));
    }

    #[test]
    fn undoing_does_not_record_and_loading_a_scene_is_not_an_edit() {
        let (mut dm, mut h) = world();
        let ws = dm.workspace;
        let part = add(&mut dm, "Part", "P", ws);
        h.commit(&mut dm);
        dm.set_prop(part, "Transparency", Value::Num(0.25)).unwrap();
        h.commit(&mut dm);
        h.undo(&mut dm);
        assert!(dm.take_journal().is_empty(), "replaying changes is not recorded");

        let place = dm.to_place("x");
        dm.load_place(&place).unwrap();
        assert!(dm.take_journal().is_empty(), "a scene load is not an edit");
        assert!(dm.journaling(), "...and recording is still on afterwards");
        dm.set_prop(named(&dm, ws, "P"), "Transparency", Value::Num(0.5)).unwrap();
        assert_eq!(dm.take_journal().len(), 1);
    }

    #[test]
    fn nothing_is_recorded_unless_journaling_is_on() {
        let mut dm = DataModel::new();
        let ws = dm.workspace;
        add(&mut dm, "Part", "P", ws);
        assert!(dm.take_journal().is_empty());
        assert!(!dm.journaling());
    }
}
