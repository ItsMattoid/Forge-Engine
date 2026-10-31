//! Editing the instance tree: insert, delete, duplicate, re-parent.

use bevy::math::{Quat, Vec3};
use forge_engine::{
    core::{class, ParentRule, PropType, ResolvedClass, Value},
    dm::{DataModel, Id},
    physics::{raycast, RayParams},
};

use super::Editor;

/// What Repeat Duplicate repeats (see [`Editor::repeat_duplicate`]): the objects the last duplication made, each with
/// the pose of its pivot when it was made.
#[derive(Clone, Debug)]
pub struct DuplicateRecord {
    pub copies: Vec<(Id, (Vec3, Quat))>,
}

impl Editor {
    fn is_container(dm: &DataModel, id: Id) -> bool {
        dm.get(id).is_some_and(|i| i.class.desc.container)
    }

    /// Where a new instance of `class` goes: the class's own `ParentRule`, applied to the selection.
    fn default_parent(&self, class: &ResolvedClass) -> Result<Id, String> {
        let dm = self.dm.borrow();
        let sel = self.selected.filter(|s| dm.exists(*s));
        let service = |n: &str| dm.service(n).ok_or_else(|| format!("missing service {n}"));
        match class.desc.parent_rule {
            ParentRule::Auto => Ok(match sel {
                Some(s) if Self::is_container(&dm, s) => s,
                _ => dm.workspace,
            }),
            ParentRule::Selected { class: want, hint } => sel.filter(|s| dm.is_a(*s, want)).ok_or_else(|| hint.to_owned()),
            ParentRule::Service(name) => service(name),
            ParentRule::SelectedOrService(name) => match sel {
                Some(s) => Ok(s),
                None => service(name),
            },
        }
    }

    /// Where new objects appear: where the editor camera looks (on the first thing it sees, else ahead of it).
    pub fn focus_point(&self) -> Vec3 {
        let (origin, dir) = self.cam_ray;
        let dm = self.dm.borrow();
        match raycast(&dm, origin, dir * 500.0, &RayParams::default()) {
            Some(h) => (h.pos + Vec3::Y * 0.5).round(),
            None => (origin + dir * 25.0).round(),
        }
    }

    pub fn insert(&mut self, class_name: &str) {
        let Some(class) = class(class_name) else { return };
        if self.project.is_none() {
            return self.warn("Open a project first.");
        }

        // Classes that carry source code get a new file with the class's starter text. The file is
        // named after the object (they start with the same name) and goes into the class's usual
        // folder; it can be moved anywhere later without changing where the script runs.
        let script_file = if class.desc.script_stub.is_some() {
            let Some(project) = &self.project else { return };
            match project.create_script(None, class_name, class_name) {
                Ok(f) => Some(f),
                Err(e) => return self.error(format!("{e:#}")),
            }
        } else {
            None
        };
        let focus = self.focus_point();

        let result: Result<Id, String> = (|| {
            let parent = self.default_parent(class)?;
            let mut dm = self.dm.borrow_mut();
            let id = dm.create(class_name)?;
            if let (Some(file), Some(rp)) = (&script_file, class.props.iter().find(|p| p.meta.ty == PropType::ScriptPath)) {
                dm.set_prop(id, rp.meta.name, Value::Str(file.clone()))?;
            }
            for (prop, default) in class.desc.insert_props {
                dm.set_prop(id, prop, default.to_value())?;
            }
            // Spatial objects appear where the editor camera looks (unless they attach to a parent).
            let attached = class.relative_transform && dm.has_component(parent, "Transform");
            if (class.desc.spawn_at_focus || class.relative_transform) && !attached && class.prop("Position").is_some() {
                dm.set_prop(id, "Position", Value::V3(focus))?;
            }
            dm.set_parent(id, Some(parent))?;
            Ok(id)
        })();
        match result {
            Ok(id) => {
                self.selected = Some(id);
                self.reveal = true;
                self.dirty = true;
                self.sync_assets(true);
            }
            Err(e) => self.warn(e),
        }
    }

    /// Deletes one object (the Explorer's context menu). If it is part of a multi-selection the
    /// whole selection goes, which is what the menu entry means then.
    pub fn delete(&mut self, id: Id) {
        self.normalize_selection();
        if self.selection.len() > 1 && self.selection.contains(&id) {
            return self.delete_selected();
        }
        let fallback = Self::inspect_fallback(&self.dm.borrow(), &[id]);
        let files = self.script_files_under(&[id]);
        self.dm.borrow_mut().destroy(id);
        self.normalize_selection();
        self.last_inspected = fallback;
        self.dirty = true;
        self.note_kept_script_files(&files);
    }

    /// Deletes everything that is selected (an object inside another selected object goes with it).
    pub fn delete_selected(&mut self) {
        self.normalize_selection();
        let targets = self.top_level_selection();
        if targets.is_empty() {
            return;
        }
        let fallback = Self::inspect_fallback(&self.dm.borrow(), &targets);
        let files = self.script_files_under(&targets);
        {
            let mut dm = self.dm.borrow_mut();
            for id in &targets {
                dm.destroy(*id);
            }
        }
        self.select_only(None);
        // The Properties panel moves to the nearest object that is still there.
        self.last_inspected = fallback;
        self.dirty = true;
        self.note_kept_script_files(&files);
    }

    pub fn duplicate(&mut self, id: Id) {
        self.normalize_selection();
        if self.selection.len() > 1 && self.selection.contains(&id) {
            return self.duplicate_selected();
        }
        if let Some(copy) = self.duplicate_one(id) {
            self.select_only(Some(copy));
            self.reveal = true;
            self.dirty = true;
            self.remember_duplicate(vec![copy]);
        }
    }

    /// Duplicates every selected object (next to the original) and selects the copies.
    pub fn duplicate_selected(&mut self) {
        self.normalize_selection();
        let copies: Vec<Id> = self.top_level_selection().into_iter().filter_map(|id| self.duplicate_one(id)).collect();
        self.select_copies(copies);
    }

    /// Selects what a duplication made (the last one is the active object), and remembers it for Repeat Duplicate.
    fn select_copies(&mut self, copies: Vec<Id>) {
        if let Some(last) = copies.last().copied() {
            self.selection = copies.clone();
            self.selected = Some(last);
            self.last_inspected = Some(last);
            self.reveal = true;
            self.dirty = true;
            self.remember_duplicate(copies);
        }
    }

    fn duplicate_one(&mut self, id: Id) -> Option<Id> {
        let mut dm = self.dm.borrow_mut();
        let parent = dm.parent(id);
        let workspace = dm.workspace;
        let copy = dm.clone_tree(id)?;
        let _ = dm.set_parent(copy, parent.or(Some(workspace)));
        Some(copy)
    }

    // ---- repeating the last duplicate -------------------------------------------------------------------

    /// Notes what a duplication made: the copies, each with the pose it was made at (its original's). What the
    /// copies are moved or turned by afterwards, with the gizmo or the Properties panel, is what Repeat Duplicate
    /// repeats.
    fn remember_duplicate(&mut self, copies: Vec<Id>) {
        let dm = self.dm.borrow();
        let copies: Vec<(Id, (Vec3, Quat))> = copies.into_iter().filter_map(|c| dm.get_pivot(c).map(|pose| (c, pose))).collect();
        drop(dm);
        self.last_duplicate = (!copies.is_empty()).then_some(DuplicateRecord { copies });
    }

    /// Forgets the last duplication (an action that makes repeating it meaningless happened).
    pub fn forget_duplicate(&mut self) {
        self.last_duplicate = None;
    }

    /// Repeat Duplicate (Ctrl+Shift+D): duplicates the last duplicate again, carrying it by the same move and turn
    /// that took it from where it was made to where it is now - so Ctrl+D, a move, then Ctrl+Shift+D again and
    /// again, makes copies the same distance apart (or round a staircase, if the copy was moved and turned).
    /// It is not a generic "repeat the last action": only duplication, and only while it still makes sense:
    ///
    /// * what is selected must be the copies of the last duplication (select something else and it is gone);
    /// * the copies must still exist, and undo, redo, Play and opening another scene or project forget it.
    ///
    /// Each repeat makes new copies, leaves the originals and earlier copies alone, selects the new ones (as Ctrl+D
    /// does) and is one step of undo. It repeats from the newest copies, so each press goes one step further.
    pub fn repeat_duplicate(&mut self) {
        if self.project.is_none() {
            return;
        }
        let Some(record) = self.last_duplicate.clone() else {
            return self.info("Nothing to repeat: duplicate something first (Ctrl+D), then move the copy.");
        };
        self.normalize_selection();
        let mut selected = self.selection_ids();
        let mut wanted: Vec<Id> = record.copies.iter().map(|(c, _)| *c).collect();
        selected.sort();
        wanted.sort();
        let alive = wanted.iter().all(|c| self.dm.borrow().exists(*c));
        if !alive || selected != wanted {
            self.last_duplicate = None;
            return self.info("Repeat Duplicate was reset: the copies are no longer selected. Duplicate again with Ctrl+D.");
        }

        let mut made: Vec<(Id, (Vec3, Quat))> = Vec::new();
        {
            let mut dm = self.dm.borrow_mut();
            let workspace = dm.workspace;
            for (copy, start) in &record.copies {
                let Some(now) = dm.get_pivot(*copy) else { continue };
                // The move and turn from where the copy was made to where it is: (turn, shift) with
                // pose' = turn * pose + shift. Applied once more to where it is now.
                let turn = (now.1 * start.1.inverse()).normalize();
                let shift = now.0 - turn * start.0;
                let target = (turn * now.0 + shift, (turn * now.1).normalize());
                let parent = dm.parent(*copy).or(Some(workspace));
                let Some(new) = dm.clone_tree(*copy) else { continue };
                if dm.set_parent(new, parent).is_err() {
                    dm.destroy(new);
                    continue;
                }
                dm.pivot_to(new, target);
                made.push((new, now));
            }
        }
        if made.is_empty() {
            return;
        }
        let ids: Vec<Id> = made.iter().map(|(id, _)| *id).collect();
        let last = *ids.last().expect("not empty");
        self.selection = ids;
        self.selected = Some(last);
        self.last_inspected = Some(last);
        self.reveal = true;
        self.dirty = true;
        // The next repeat measures from these, against where they were made (where the copies they came from are).
        self.last_duplicate = Some(DuplicateRecord { copies: made });
    }
    /// Starts choosing a new parent for `targets`: the next click on an object in the Explorer or the
    /// viewport makes it their parent. Services cannot be moved and are left out.
    pub fn begin_parent_pick(&mut self, targets: Vec<Id>) {
        let dm = self.dm.borrow();
        let movable: Vec<Id> = targets.into_iter().filter(|t| dm.get(*t).is_some_and(|i| i.class.desc.service_order.is_none() && !i.class.desc.root)).collect();
        drop(dm);
        if !movable.is_empty() {
            self.picking_primary = None;
            self.picking_parent = Some(movable);
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn cancel_parent_pick(&mut self) {
        self.picking_parent = None;
    }

    // ---- choosing a model's primary part (the same way as a parent, with a stricter filter) -----------------

    /// Starts choosing the `PrimaryPart` of `model`: the next click on an object in the Explorer or the viewport
    /// picks it, like choosing a parent - but only a part inside the model itself is taken (see
    /// `DataModel::primary_part_problem`, the rule the data layer enforces too).
    pub fn begin_primary_pick(&mut self, model: Id) {
        let dm = self.dm.borrow();
        if !dm.get(model).is_some_and(|i| i.class.prop("PrimaryPart").is_some()) {
            return;
        }
        let has_parts = dm.subtree(model).into_iter().skip(1).any(|n| dm.primary_part_problem(model, n).is_none());
        let name = dm.name(model);
        drop(dm);
        if !has_parts {
            return self.warn(format!("{name} has no part inside it that could be its primary part. Put a part in it first."));
        }
        self.picking_parent = None;
        self.picking_primary = Some(model);
    }

    /// Whether `candidate` can be the primary part being picked: a part inside the model.
    pub fn primary_pick_ok(&self, candidate: Id) -> bool {
        let Some(model) = self.picking_primary else { return false };
        self.dm.borrow().primary_part_problem(model, candidate).is_none()
    }

    /// The click that finishes choosing a primary part. Returns whether it was applied; something that cannot be
    /// the primary part is explained and the choosing goes on.
    pub fn pick_primary_part(&mut self, picked: Id) -> bool {
        let Some(model) = self.picking_primary else { return false };
        let problem = self.dm.borrow().primary_part_problem(model, picked);
        if let Some(why) = problem {
            let name = self.dm.borrow().name(picked);
            self.warn(format!("{name} cannot be the primary part: {why}. Pick a part inside the model, or press Esc."));
            return false;
        }
        self.picking_primary = None;
        let result = self.dm.borrow_mut().set_prop(model, "PrimaryPart", Value::Ref(Some(picked)));
        match result {
            Ok(()) => {
                self.dirty = true;
                let (model_name, part_name) = (self.dm.borrow().name(model), self.dm.borrow().name(picked));
                self.info(format!("The primary part of {model_name} is now {part_name}."));
            }
            Err(e) => self.warn(e),
        }
        true
    }

    // ---- choosing: either of them ---------------------------------------------------------------------------

    /// Whether a parent or a primary part is being chosen (the next click on an object is the choice).
    pub fn picking_any(&self) -> bool {
        self.picking_parent.is_some() || self.picking_primary.is_some()
    }

    /// Whether `candidate` is something the choice in progress accepts.
    pub fn pick_ok(&self, candidate: Id) -> bool {
        if self.picking_primary.is_some() {
            self.primary_pick_ok(candidate)
        } else {
            self.parent_pick_ok(candidate)
        }
    }

    /// The click that makes the choice in progress. Returns whether it was applied.
    pub fn pick_candidate(&mut self, picked: Id) -> bool {
        if self.picking_primary.is_some() {
            self.pick_primary_part(picked)
        } else {
            self.pick_parent(picked)
        }
    }

    /// Gives up the choice in progress (Esc, or the Cancel button).
    pub fn cancel_pick(&mut self) {
        self.picking_parent = None;
        self.picking_primary = None;
    }

    /// Whether candidate can be the parent being picked: not one of the objects being moved, nor inside one.
    pub fn parent_pick_ok(&self, candidate: Id) -> bool {
        let Some(targets) = &self.picking_parent else { return false };
        let dm = self.dm.borrow();
        dm.exists(candidate) && !dm.get(candidate).is_some_and(|i| i.class.desc.root) && !targets.iter().any(|t| *t == candidate || dm.is_descendant_of(candidate, *t))
    }

    /// The click that finishes choosing a parent. Returns whether it was applied; a candidate that cannot
    /// be the parent (the object itself, or something inside it) is explained and the choosing goes on.
    pub fn pick_parent(&mut self, picked: Id) -> bool {
        let Some(targets) = self.picking_parent.clone() else { return false };
        if !self.parent_pick_ok(picked) {
            let name = self.dm.borrow().name(picked);
            self.warn(format!("{name} cannot be the parent: an object cannot go inside itself or something inside it. Pick another, or press Esc."));
            return false;
        }
        self.picking_parent = None;
        let mut moved = Vec::new();
        for target in &targets {
            if self.dm.borrow().parent(*target) == Some(picked) {
                continue;
            }
            let result = self.dm.borrow_mut().set_parent(*target, Some(picked));
            match result {
                Ok(()) => moved.push(*target),
                Err(e) => self.warn(e),
            }
        }
        let where_to = self.dm.borrow().name(picked);
        if moved.is_empty() {
            self.info(format!("Nothing moved: already inside {where_to}."));
            return true;
        }
        self.dirty = true;
        self.expand_to(moved[0]);
        self.info(format!("Moved {} object(s) into {where_to}", moved.len()));
        true
    }

    /// Moves an object under a new parent (drag and drop in the Explorer). Dragging one of several
    /// selected objects moves all of them.
    pub fn reparent(&mut self, id: Id, new_parent: Id) {
        self.normalize_selection();
        let moving: Vec<Id> = if self.selection.len() > 1 && self.selection.contains(&id) { self.top_level_selection() } else { vec![id] };
        let mut moved = false;
        for item in moving {
            if item == new_parent {
                continue;
            }
            let r = self.dm.borrow_mut().set_parent(item, Some(new_parent));
            match r {
                Ok(()) => moved = true,
                Err(e) => self.warn(e),
            }
        }
        if moved {
            self.dirty = true;
            // The objects stay selected; open the new parent so they are not hidden inside a closed node.
            self.expand_to(id);
        }
    }
}