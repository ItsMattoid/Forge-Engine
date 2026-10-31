//! What is selected.
//!
//! `Editor::selection` is every selected object and `Editor::selected` is the *active* one: the last
//! one picked, the one the gizmo sits on and whose properties the panel leads with. Code that only
//! cares about one object keeps reading `selected`; code that works on all of them reads
//! `selection`. The two are kept consistent by [`Editor::normalize_selection`], which also copes
//! with code that simply assigns `selected` (that means "select only this", or "select nothing").
//!
//! The Properties panel can keep showing the last object that was selected after the selection is
//! cleared (`last_inspected`); that is a display target, not a selection.

use forge_engine::dm::{DataModel, Id};

use super::Editor;

impl Editor {
    /// Makes `selection` and `selected` agree and drops objects that no longer exist.
    pub fn normalize_selection(&mut self) {
        let dm = self.dm.clone();
        let dm = dm.borrow();
        self.selection.retain(|id| dm.exists(*id));
        match self.selected {
            Some(active) if dm.exists(active) => {
                // Assigned directly (not through the helpers): it becomes the only selected object.
                if !self.selection.contains(&active) {
                    self.selection = vec![active];
                }
            }
            Some(_) => self.selected = self.selection.last().copied(),
            None => self.selection.clear(),
        }
        if let Some(active) = self.selected {
            self.last_inspected = Some(active);
        }
        if self.last_inspected.is_some_and(|id| !dm.exists(id)) {
            self.last_inspected = None;
        }
    }

    pub fn is_selected(&self, id: Id) -> bool {
        self.selection.contains(&id) || self.selected == Some(id)
    }

    /// Selects exactly this object (or nothing).
    pub fn select_only(&mut self, id: Option<Id>) {
        self.selection = id.into_iter().collect();
        self.selected = id;
        if id.is_some() {
            self.last_inspected = id;
        }
    }

    /// Adds an object to the selection and makes it the active one.
    pub fn select_add(&mut self, id: Id) {
        self.normalize_selection();
        if !self.selection.contains(&id) {
            self.selection.push(id);
        }
        self.selected = Some(id);
        self.last_inspected = Some(id);
    }

    /// Shift / Alt click: adds the object, or takes it out if it is already selected.
    pub fn select_toggle(&mut self, id: Id) {
        self.normalize_selection();
        if self.selection.contains(&id) {
            self.selection.retain(|s| *s != id);
            if self.selected == Some(id) {
                self.selected = self.selection.last().copied();
            }
        } else {
            self.select_add(id);
        }
    }

    /// The selected objects that are not inside another selected object: what moving, rotating,
    /// copying and deleting act on (a child moves with its parent, so it must not be moved twice).
    pub fn top_level_selection(&self) -> Vec<Id> {
        let dm = self.dm.borrow();
        let ids = self.selection_ids();
        let chosen: std::collections::HashSet<Id> = ids.iter().copied().collect();
        // Walks each object's ancestors (not every pair of objects), so a selection of thousands is quick.
        ids.into_iter()
            .filter(|id| {
                let mut cur = dm.parent(*id);
                while let Some(p) = cur {
                    if chosen.contains(&p) {
                        return false;
                    }
                    cur = dm.parent(p);
                }
                true
            })
            .collect()
    }

    /// The selected objects (the active one included), as far as they exist.
    pub fn selection_ids(&self) -> Vec<Id> {
        let dm = self.dm.borrow();
        let mut ids: Vec<Id> = self.selection.iter().copied().filter(|id| dm.exists(*id)).collect();
        if let Some(active) = self.selected.filter(|a| dm.exists(*a)) {
            if !ids.contains(&active) {
                ids = vec![active];
            }
        }
        ids
    }

    /// What is selected right now, as the history stores it.
    pub fn selection_state(&mut self) -> SelectionState {
        self.normalize_selection();
        SelectionState { items: self.selection.clone(), active: self.selected }
    }

    /// Puts a recorded selection back (undo / redo). Objects that no longer exist are dropped.
    pub fn apply_selection_state(&mut self, state: &SelectionState) {
        let dm = self.dm.borrow();
        self.selection = state.items.iter().copied().filter(|id| dm.exists(*id)).collect();
        self.selected = state.active.filter(|a| dm.exists(*a) && self.selection.contains(a)).or_else(|| self.selection.last().copied());
        if let Some(active) = self.selected {
            self.last_inspected = Some(active);
        }
    }

    /// The object whose properties the panel shows, and whether it is currently selected.

    pub fn inspected(&self) -> Option<(Id, bool)> {
        let dm = self.dm.borrow();
        if let Some(active) = self.selected.filter(|a| dm.exists(*a)) {
            return Some((active, true));
        }
        self.last_inspected.filter(|id| dm.exists(*id)).map(|id| (id, false))
    }

    /// The title of the Properties window: `Properties - Part "SpawnLocation"` for one selected object,
    /// `Properties - Multiple Objects` for several, plain `Properties` for none (the last object the panel
    /// keeps on show after the selection is cleared is not selected, so it is not named here). Read from the
    /// data model every time, so a rename or a class change shows at once.
    pub fn properties_title(&self) -> String {
        let ids = self.selection_ids();
        match ids.as_slice() {
            [] => "Properties".to_owned(),
            [only] => {
                let dm = self.dm.borrow();
                // (A name with a line break would break the tab; it is shown on one line.)
                let name = dm.name(*only).replace(['\n', '\r'], " ");
                format!("Properties - {} \"{name}\"", dm.class_name(*only))
            }
            _ => "Properties - Multiple Objects".to_owned(),
        }
    }

    /// Where the Properties panel should look after `ids` were deleted: the nearest ancestor of the
    /// first one that is not a service.
    pub(super) fn inspect_fallback(dm: &DataModel, ids: &[Id]) -> Option<Id> {
        let first = ids.first()?;
        let mut cur = dm.parent(*first);
        while let Some(p) = cur {
            if dm.get(p).is_some_and(|i| i.class.desc.service_order.is_none() && !i.class.desc.root) && !ids.contains(&p) {
                return Some(p);
            }
            cur = dm.parent(p);
        }
        None
    }
}

/// Everything about what is selected, as the undo history stores it: the objects and which of them is active.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionState {
    pub items: Vec<Id>,
    pub active: Option<Id>,
}

/// Alt-clicking the same spot again and again walks through everything behind one another there.
#[derive(Default, Debug)]
pub struct PickCycle {
    pos: bevy::math::Vec2,
    /// What was under the mouse (nearest first) when the walk began.
    ids: Vec<Id>,
    /// Which of them was chosen last.
    next: usize,
}

impl PickCycle {
    /// The object a click at `pos` chooses, given everything under the mouse nearest first. The
    /// first click picks the nearest; each further click at (about) the same spot, seeing the same
    /// objects, picks the next one behind it and wraps round after the last. Moving the mouse more
    /// than `tolerance` pixels, or a different set of objects under it, starts over.
    pub fn advance(&mut self, pos: bevy::math::Vec2, hits: &[Id], tolerance: f32) -> Option<Id> {
        if hits.is_empty() {
            self.reset();
            return None;
        }
        let continuing = !self.ids.is_empty() && self.pos.distance(pos) <= tolerance && self.ids == hits;
        if continuing {
            self.next = (self.next + 1) % hits.len();
        } else {
            self.ids = hits.to_vec();
            self.pos = pos;
            self.next = 0;
        }
        hits.get(self.next).copied()
    }

    pub fn reset(&mut self) {
        self.ids.clear();
        self.next = 0;
    }
}

impl Editor {
    /// What a plain click on `id` in the viewport selects: the outermost Model it is inside (a group is picked as
    /// a whole), or `id` itself when it is not in one. Folders are only containers and are never picked this way.
    /// (Alt-clicking skips this and picks the part itself.)
    pub fn pick_target(&self, id: Id) -> Id {
        let dm = self.dm.borrow();
        let mut found = id;
        let mut cur = dm.parent(id);
        while let Some(p) = cur {
            if dm.get(p).is_some_and(|i| i.class.is_a("Model")) {
                found = p;
            }
            cur = dm.parent(p);
        }
        found
    }

    /// A click in the viewport on whatever is under the mouse (`hits`, nearest first):
    /// * plain: select the nearest - or the whole group (Model) it is in - or clear the selection when there is nothing;
    /// * Shift: add that to the selection, or take it out if it is already selected;
    /// * Alt: select the nearest part itself, even inside a group, and on repeated clicks at the same spot the next
    ///   one behind it;
    /// * Shift + Alt: the same walk, but each object is added to the selection.
    pub fn click_select(&mut self, hits: &[Id], pos: bevy::math::Vec2, shift: bool, alt: bool) {
        let tolerance = self.settings.click_cycle_tolerance;
        match (shift, alt) {
            (false, false) => {
                self.pick_cycle.reset();
                let target = hits.first().map(|h| self.pick_target(*h));
                self.select_only(target);
            }
            (true, false) => {
                self.pick_cycle.reset();
                if let Some(first) = hits.first() {
                    let target = self.pick_target(*first);
                    self.select_toggle(target);
                }
            }
            (false, true) => {
                let chosen = self.pick_cycle.advance(pos, hits, tolerance);
                self.select_only(chosen);
            }
            (true, true) => {
                if let Some(chosen) = self.pick_cycle.advance(pos, hits, tolerance) {
                    self.select_add(chosen);
                }
            }
        }
        self.reveal = self.selected.is_some();
    }
}
#[cfg(test)]
mod tests {
    use forge_engine::project::Project;

    use super::*;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_selection_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Selection Test").expect("create project");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn editor(s: &Scratch) -> Editor {
        let mut ed = Editor::new(Some(s.0.clone()));
        ed.commit_edits(false);
        ed.history.clear();
        ed
    }

    fn find(ed: &Editor, name: &str) -> Id {
        let dm = ed.dm.borrow();
        dm.find_first_child(dm.workspace, name, true).unwrap_or_else(|| panic!("no '{name}'"))
    }

    #[test]
    fn helpers_keep_the_active_object_and_the_list_in_step() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b, c) = (find(&ed, "Wall"), find(&ed, "Step 1"), find(&ed, "Step 2"));

        ed.select_only(Some(a));
        assert_eq!((ed.selected, ed.selection.clone()), (Some(a), vec![a]));
        ed.select_toggle(b);
        ed.select_toggle(c);
        assert_eq!(ed.selection, vec![a, b, c]);
        assert_eq!(ed.selected, Some(c), "the last one picked is the active one");

        ed.select_toggle(c);
        assert_eq!(ed.selection, vec![a, b]);
        assert_eq!(ed.selected, Some(b), "taking the active one out hands over to the previous one");
        ed.select_toggle(a);
        ed.select_toggle(b);
        assert!(ed.selection.is_empty() && ed.selected.is_none());
        assert_eq!(ed.inspected(), Some((b, false)), "the panel keeps showing the last object, marked as not selected");
    }

    #[test]
    fn the_properties_title_names_one_selected_object_and_follows_renames_and_the_selection() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (wall, step) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        assert_eq!(ed.properties_title(), "Properties", "nothing selected");

        ed.select_only(Some(wall));
        let class = ed.dm.borrow().class_name(wall);
        assert_eq!(ed.properties_title(), format!("Properties - {class} \"Wall\""));

        ed.dm.borrow_mut().set_name(wall, "SpawnLocation");
        assert_eq!(ed.properties_title(), format!("Properties - {class} \"SpawnLocation\""), "a rename shows at once");

        ed.select_toggle(step);
        assert_eq!(ed.properties_title(), "Properties - Multiple Objects", "never one name for several objects");
        ed.select_toggle(step);
        assert_eq!(ed.properties_title(), format!("Properties - {class} \"SpawnLocation\""), "back to one");

        ed.select_toggle(wall);
        assert_eq!(ed.inspected(), Some((wall, false)), "the panel still shows the last object...");
        assert_eq!(ed.properties_title(), "Properties", "...but nothing is selected, so the title does not name it");

        ed.select_only(Some(wall));
        ed.dm.borrow_mut().set_name(wall, "Two\nLines");
        assert_eq!(ed.properties_title(), format!("Properties - {class} \"Two Lines\""), "kept on one line");
        ed.dm.borrow_mut().destroy(wall);
        assert_eq!(ed.properties_title(), "Properties", "a deleted object is not named");
    }

    #[test]
    fn assigning_selected_directly_still_means_select_only_this_or_nothing() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        ed.select_only(Some(a));
        ed.select_toggle(b);

        ed.selected = Some(a);
        ed.normalize_selection();
        assert_eq!(ed.selection, vec![a, b], "making a selected object active keeps the selection");

        let outsider = find(&ed, "Step 2");
        ed.selected = Some(outsider);
        ed.normalize_selection();
        assert_eq!(ed.selection, vec![outsider], "assigning an object that was not selected means select only it");

        ed.select_toggle(b);
        ed.selected = None;
        ed.normalize_selection();
        assert!(ed.selection.is_empty(), "assigning None clears everything");
    }

    #[test]
    fn deleted_objects_leave_the_selection_and_the_panel_falls_back_safely() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        ed.select_only(Some(a));
        ed.select_toggle(b);
        ed.dm.borrow_mut().destroy(b);
        ed.normalize_selection();
        assert_eq!((ed.selected, ed.selection.clone()), (Some(a), vec![a]), "the active object was deleted: the other one takes over");

        ed.dm.borrow_mut().destroy(a);
        ed.normalize_selection();
        assert!(ed.selected.is_none() && ed.selection.is_empty());
        assert_eq!(ed.inspected(), None, "nothing valid left to show");
    }

    #[test]
    fn a_selected_child_is_not_moved_twice() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = find(&ed, "Player");
        let root = ed.dm.borrow().find_child(model, "Body").unwrap();
        let wall = find(&ed, "Wall");
        ed.select_only(Some(model));
        ed.select_toggle(root);
        ed.select_toggle(wall);
        let top = ed.top_level_selection();
        assert_eq!(top.len(), 2);
        assert!(top.contains(&model) && top.contains(&wall) && !top.contains(&root));
    }

    #[test]
    fn plain_and_shift_clicks_select_and_toggle() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        let at = bevy::math::Vec2::new(100.0, 100.0);

        ed.click_select(&[a], at, false, false);
        assert_eq!(ed.selection, vec![a]);
        ed.click_select(&[b], at, true, false);
        assert_eq!(ed.selection, vec![a, b], "shift adds");
        ed.click_select(&[a], at, true, false);
        assert_eq!(ed.selection, vec![b], "shift on a selected object takes it out and keeps the rest");
        ed.click_select(&[], at, true, false);
        assert_eq!(ed.selection, vec![b], "shift-clicking empty space changes nothing");
        ed.click_select(&[a, b], at, false, false);
        assert_eq!(ed.selection, vec![a], "a plain click picks the nearest and drops the others");
        ed.click_select(&[], at, false, false);
        assert!(ed.selection.is_empty() && ed.selected.is_none(), "clicking empty space clears");
    }

    #[test]
    fn alt_clicks_walk_through_what_is_behind_and_wrap() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (front, middle, back) = (find(&ed, "Wall"), find(&ed, "Step 1"), find(&ed, "Step 2"));
        let stack = [front, middle, back];
        let at = bevy::math::Vec2::new(300.0, 200.0);

        ed.click_select(&stack, at, false, true);
        assert_eq!(ed.selected, Some(front));
        ed.click_select(&stack, at + bevy::math::Vec2::new(1.5, 0.0), false, true);
        assert_eq!(ed.selected, Some(middle), "the same spot again: the next one back (a pixel or two of drift is fine)");
        ed.click_select(&stack, at, false, true);
        assert_eq!(ed.selected, Some(back));
        assert_eq!(ed.selection, vec![back], "plain Alt replaces the selection");
        ed.click_select(&stack, at, false, true);
        assert_eq!(ed.selected, Some(front), "after the last it starts again");

        // Moving the mouse far enough starts over from the nearest.
        ed.click_select(&stack, at, false, true);
        ed.click_select(&stack, at + bevy::math::Vec2::new(40.0, 0.0), false, true);
        assert_eq!(ed.selected, Some(front));
        // So does a different set of objects under the mouse.
        ed.click_select(&stack, at, false, true);
        ed.click_select(&[middle, back], at, false, true);
        assert_eq!(ed.selected, Some(middle));
    }

    #[test]
    fn shift_alt_clicks_walk_and_keep_adding() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (other, front, middle) = (find(&ed, "Step 3"), find(&ed, "Wall"), find(&ed, "Step 1"));
        let stack = [front, middle];
        let at = bevy::math::Vec2::new(10.0, 10.0);
        ed.select_only(Some(other));

        ed.click_select(&stack, at, true, true);
        assert_eq!(ed.selection, vec![other, front], "the existing selection is kept");
        ed.click_select(&stack, at, true, true);
        assert_eq!(ed.selection, vec![other, front, middle], "the next object behind is added too");
        assert_eq!(ed.selected, Some(middle), "and it is the active one");
        ed.click_select(&stack, at, true, true);
        assert_eq!(ed.selection, vec![other, front, middle], "wrapping round adds nothing new");
        assert_eq!(ed.selected, Some(front));
    }

    #[test]
    fn the_cycle_resets_when_nothing_is_under_the_mouse() {
        let mut cycle = PickCycle::default();
        let (a, b) = (Id { slot: 1, uid: 1 }, Id { slot: 2, uid: 2 });
        let at = bevy::math::Vec2::ZERO;
        assert_eq!(cycle.advance(at, &[a, b], 3.0), Some(a));
        assert_eq!(cycle.advance(at, &[], 3.0), None);
        assert_eq!(cycle.advance(at, &[a, b], 3.0), Some(a), "after empty space the walk starts over");
        assert_eq!(cycle.advance(at, &[a, b], 3.0), Some(b));
    }}
