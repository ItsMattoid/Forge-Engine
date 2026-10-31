//! Command-based undo / redo, selection included.
//!
//! The data model records every edit it is given as a reversible change (see
//! `forge_engine::dm::journal`). At the end of each user action [`Editor::commit_edits`] takes
//! those changes and keeps them as one [`Command`]. Undo replays a command's changes backwards,
//! redo forwards. Only what actually changed is touched, instances stay the same instances (a
//! deleted one comes back under a new id, which [`History`] tracks), and the cost of an edit does
//! not depend on how big the scene is.
//!
//! What is selected is part of the same history. A command carries the selection before and after
//! it when the selection changed: a click on an object is a step of its own (nothing edited, only the
//! selection), and an edit that also changed the selection (a paste selects what it pasted, a delete
//! clears the selection) undoes both together. Undo puts back the complete previous selection: the
//! same objects, the same active one.

use std::time::{Duration, Instant};

use forge_engine::dm::{
    file::PlaceFile,
    journal::{Change, IdMap},
    DataModel, Id,
};

use super::{Editor, SelectionState};

/// One undoable user action.
struct Command {
    changes: Vec<Change>,
    /// The selection before and after, when this step changed it.
    selection: Option<(SelectionState, SelectionState)>,
    /// When the last change was added (typing and dragging keep extending one command).
    at: Instant,
}

impl Command {
    /// Only property edits and nothing else: something that can absorb more edits of the same properties.
    fn is_property_edit(&self) -> bool {
        self.selection.is_none() && !self.changes.is_empty() && self.changes.iter().all(|c| matches!(c, Change::Prop { .. }))
    }

    /// Folds `later` (also only property edits) into this command if it edits exactly the
    /// same properties; then undo steps over the whole burst (a typed number, a slider drag
    /// that was released and grabbed again) at once.
    fn absorb(&mut self, later: &[Change], now: Instant) -> bool {
        let same_targets = later.len() == self.changes.len()
            && later.iter().all(|l| {
                let Change::Prop { id, name, .. } = l else { return false };
                self.changes.iter().any(|c| matches!(c, Change::Prop { id: i, name: n, .. } if i == id && n == name))
            });
        if !same_targets {
            return false;
        }
        for l in later {
            let Change::Prop { id, name, new, .. } = l else { continue };
            for c in &mut self.changes {
                if let Change::Prop { id: i, name: n, new: current, .. } = c {
                    if i == id && n == name {
                        *current = new.clone();
                    }
                }
            }
        }
        self.at = now;
        true
    }

    /// True when every property write in the command ended where it started and the selection did not change.
    fn is_noop(&self) -> bool {
        self.selection.is_none() && self.changes.iter().all(|c| matches!(c, Change::Prop { old, new, .. } if old == new))
    }
}

/// What an undo or redo did, for the editor to react to.
pub struct Applied {
    /// The object the command was about (the Explorer scrolls to it if it is selected).
    pub focus: Option<Id>,
    /// Steps that could not be replayed (the object they touched is gone).
    pub problems: Vec<String>,
    /// The selection to put back, when the step changed it (its ids already point at the live objects).
    pub selection: Option<SelectionState>,
    /// Whether the step edited the scene (as opposed to only the selection).
    pub edited: bool,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Command>,
    redo: Vec<Command>,
    /// Old id -> current id, for instances that were deleted and brought back.
    ids: IdMap,
}

impl History {
    const LIMIT: usize = 1000;
    /// Edits of the same properties closer together than this are one undo step.
    const MERGE_WITHIN: Duration = Duration::from_millis(600);

    /// Adds a finished user action: the edits it made and, if it changed it, the selection (before, after).
    /// Starting a new action ends the chance to redo.
    pub fn record(&mut self, changes: Vec<Change>, selection: Option<(SelectionState, SelectionState)>, now: Instant) {
        if changes.is_empty() && selection.is_none() {
            return;
        }
        let new = Command { changes, selection, at: now };
        if new.is_property_edit() {
            if let Some(last) = self.undo.last_mut() {
                if last.is_property_edit() && now.duration_since(last.at) < Self::MERGE_WITHIN && last.absorb(&new.changes, now) {
                    self.redo.clear();
                    if last.is_noop() {
                        self.undo.pop();
                    }
                    return;
                }
            }
        }
        self.redo.clear();
        self.undo.push(new);
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
    }

    /// `state` with every id followed to the instance that stands for it now.
    pub fn resolve_state(&self, state: &SelectionState) -> SelectionState {
        SelectionState { items: state.items.iter().map(|id| self.ids.resolve(*id)).collect(), active: state.active.map(|id| self.ids.resolve(id)) }
    }

    pub fn undo(&mut self, dm: &mut DataModel) -> Option<Applied> {
        let mut cmd = self.undo.pop()?;
        let mut problems = Vec::new();
        for change in cmd.changes.iter_mut().rev() {
            if let Err(e) = dm.undo_change(change, &mut self.ids) {
                problems.push(e);
            }
        }
        let focus = cmd.changes.first().and_then(Change::focus).map(|id| self.ids.resolve(id));
        let selection = cmd.selection.as_ref().map(|(before, _)| self.resolve_state(before));
        let edited = !cmd.changes.is_empty();
        self.redo.push(cmd);
        Some(Applied { focus, problems, selection, edited })
    }

    pub fn redo(&mut self, dm: &mut DataModel) -> Option<Applied> {
        let mut cmd = self.redo.pop()?;
        let mut problems = Vec::new();
        for change in cmd.changes.iter_mut() {
            if let Err(e) = dm.redo_change(change, &mut self.ids) {
                problems.push(e);
            }
        }
        let focus = cmd.changes.first().and_then(Change::focus).map(|id| self.ids.resolve(id));
        let selection = cmd.selection.as_ref().map(|(_, after)| self.resolve_state(after));
        let edited = !cmd.changes.is_empty();
        self.undo.push(cmd);
        Some(Applied { focus, problems, selection, edited })
    }

    /// Forgets everything (a different scene was opened).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.ids.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The scene was rebuilt from a snapshot (a game stopped): the instance at position `i` of
    /// `before` is now the one at position `i` of `after`. The history survives, pointing at the new ones.
    pub fn remap(&mut self, before: &[Id], after: &[Id]) {
        for (old, new) in before.iter().zip(after) {
            self.ids.link(*old, *new);
        }
    }
}
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use forge_engine::{core::Value, project::Project};

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_undo_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Undo Test").expect("create project");
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
        assert!(ed.project.is_some());
        ed.commit_edits(false);
        ed.history.clear();
        ed
    }

    fn wall(ed: &Editor) -> Id {
        let dm = ed.dm.borrow();
        dm.find_first_child(dm.workspace, "Wall", false).expect("the template has a Wall")
    }

    fn transparency(ed: &Editor, id: Id) -> f32 {
        ed.dm.borrow().num(id, "Transparency", -1.0)
    }

    #[test]
    fn inserting_undoes_and_redoes_and_selects_the_object() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let parts_before = ed.dm.borrow().subtree(ed.dm.borrow().workspace).len();

        ed.insert("Part");
        ed.commit_edits(false);
        let inserted = ed.selected.expect("the new part is selected");
        assert_eq!(ed.dm.borrow().subtree(ed.dm.borrow().workspace).len(), parts_before + 1);

        ed.undo();
        assert!(!ed.dm.borrow().exists(inserted));
        assert_eq!(ed.dm.borrow().subtree(ed.dm.borrow().workspace).len(), parts_before);
        assert!(ed.selected.is_none(), "nothing is left selected that no longer exists");

        ed.redo();
        assert_eq!(ed.dm.borrow().subtree(ed.dm.borrow().workspace).len(), parts_before + 1);
        assert!(ed.selected.is_some_and(|id| ed.dm.borrow().exists(id)), "the re-created part is selected");
        assert!(ed.dirty);
    }

    #[test]
    fn undo_does_not_rebuild_what_it_did_not_change() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = wall(&ed);
        let other = {
            let dm = ed.dm.borrow();
            dm.find_first_child(dm.workspace, "Step 1", false).unwrap()
        };

        ed.dm.borrow_mut().set_prop(wall, "Transparency", Value::Num(0.5)).unwrap();
        ed.commit_edits(false);
        ed.undo();
        assert_eq!(transparency(&ed, wall), 0.0);
        assert!(ed.dm.borrow().exists(wall) && ed.dm.borrow().exists(other), "the same instances, not rebuilt copies");
    }

    #[test]
    fn typing_a_value_is_one_undo_step_and_a_held_mouse_is_one_gesture() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = wall(&ed);

        // A slider drag: the button stays down for several frames, nothing is committed until it is released.
        for v in [0.1, 0.2, 0.3] {
            ed.dm.borrow_mut().set_prop(wall, "Transparency", Value::Num(v)).unwrap();
            ed.commit_edits(true);
        }
        assert!(!ed.history.can_undo(), "still dragging");
        ed.commit_edits(false);
        assert!(ed.history.can_undo());

        // Typing: a change per frame, no mouse involved; they merge.
        for v in [0.4, 0.5, 0.6] {
            ed.dm.borrow_mut().set_prop(wall, "Transparency", Value::Num(v)).unwrap();
            ed.commit_edits(false);
        }
        ed.undo();
        assert_eq!(transparency(&ed, wall), 0.0, "drag and typing on the same property are one step");
        assert!(!ed.history.can_undo());
        ed.redo();
        assert_eq!(transparency(&ed, wall), 0.6);
    }

    #[test]
    fn delete_then_undo_brings_the_object_back_selected_with_its_children() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = {
            let dm = ed.dm.borrow();
            dm.find_first_child(dm.workspace, "Player", false).expect("the player model")
        };
        let before = ed.dm.borrow().subtree(model).len();
        assert!(before > 1);

        ed.select_only(Some(model));
        ed.commit_edits(false);
        ed.delete_selected();
        ed.commit_edits(false);
        assert!(ed.dm.borrow().find_first_child(ed.dm.borrow().workspace, "Player", false).is_none());
        assert!(ed.selection.is_empty(), "deleting clears the selection");

        ed.undo();
        let back = ed.dm.borrow().find_first_child(ed.dm.borrow().workspace, "Player", false).expect("the model is back");
        assert_eq!(ed.dm.borrow().subtree(back).len(), before, "with all its children");
        assert_eq!(ed.selected, Some(back));
        let root = ed.dm.borrow().find_child(back, "Body").unwrap();
        assert_eq!(ed.dm.borrow().reference(back, "PrimaryPart"), Some(root), "and its PrimaryPart reference");
    }

    #[test]
    fn the_history_survives_a_play_session_and_play_edits_are_not_in_it() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall_id = wall(&ed);
        ed.dm.borrow_mut().set_prop(wall_id, "Transparency", Value::Num(0.5)).unwrap();
        ed.commit_edits(false);

        ed.start_play();
        assert!(ed.playing());
        let live_wall = wall(&ed);
        ed.dm.borrow_mut().set_prop(live_wall, "Transparency", Value::Num(0.9)).unwrap();
        ed.commit_edits(false);
        ed.stop_play();

        let wall_now = wall(&ed);
        assert_ne!(wall_now, wall_id, "Stop rebuilt the scene");
        assert_eq!(transparency(&ed, wall_now), 0.5, "the live edit is gone");

        ed.undo();
        assert_eq!(transparency(&ed, wall_now), 0.0, "undo reaches the rebuilt instance, and undoes the pre-Play edit, not the live one");
        assert!(!ed.history.can_undo());
    }

    #[test]
    fn opening_a_scene_starts_a_fresh_history() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        ed.insert("Part");
        ed.commit_edits(false);
        assert!(ed.history.can_undo());
        ed.load_scene("Scenes/MainScene.scene");
        assert!(!ed.history.can_undo());
        assert!(ed.dm.borrow().journaling(), "and recording continues");
    }

    // ---- selection is part of the history ------------------------------------------------------------------------

    fn step(ed: &mut Editor) {
        ed.commit_edits(false);
    }

    #[test]
    fn selecting_and_deselecting_can_be_undone_and_redone() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (wall(&ed), {
            let dm = ed.dm.borrow();
            dm.find_first_child(dm.workspace, "Step 1", false).unwrap()
        });

        ed.select_only(Some(a));
        step(&mut ed);
        ed.select_only(Some(b));
        step(&mut ed);
        ed.select_only(None);
        step(&mut ed);

        ed.undo();
        assert_eq!(ed.selection, vec![b], "deselecting is undone");
        ed.undo();
        assert_eq!((ed.selection.clone(), ed.selected), (vec![a], Some(a)));
        ed.undo();
        assert!(ed.selection.is_empty() && ed.selected.is_none(), "back to nothing selected");
        assert!(!ed.history.can_undo());
        assert!(!ed.dirty, "selecting is not an edit of the scene");

        ed.redo();
        assert_eq!(ed.selected, Some(a));
        ed.redo();
        ed.redo();
        assert!(ed.selection.is_empty());
        assert!(!ed.history.can_redo());
    }

    #[test]
    fn the_whole_selection_and_the_active_object_come_back() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let ids: Vec<Id> = ["Wall", "Step 1", "Step 2"].iter().map(|n| ed.dm.borrow().find_first_child(ed.dm.borrow().workspace, n, true).unwrap()).collect();
        let (a, b, c) = (ids[0], ids[1], ids[2]);

        ed.select_only(Some(a));
        step(&mut ed);
        ed.select_toggle(b);
        step(&mut ed);
        ed.select_toggle(c);
        step(&mut ed);
        // Only the active object changes: make `a` the active one without changing what is selected.
        ed.selected = Some(a);
        step(&mut ed);
        assert_eq!((ed.selection.clone(), ed.selected), (vec![a, b, c], Some(a)));

        ed.undo();
        assert_eq!((ed.selection.clone(), ed.selected), (vec![a, b, c], Some(c)), "the active object is restored too");
        ed.undo();
        assert_eq!((ed.selection.clone(), ed.selected), (vec![a, b], Some(b)));
        ed.undo();
        assert_eq!((ed.selection.clone(), ed.selected), (vec![a], Some(a)));
        ed.redo();
        ed.redo();
        ed.redo();
        assert_eq!((ed.selection.clone(), ed.selected), (vec![a, b, c], Some(a)));
    }

    #[test]
    fn selection_steps_and_edits_stay_separate_and_in_order() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (wall(&ed), ed.dm.borrow().find_first_child(ed.dm.borrow().workspace, "Step 1", false).unwrap());

        ed.select_only(Some(a));
        step(&mut ed); // 1: select a
        ed.dm.borrow_mut().set_prop(a, "Transparency", Value::Num(0.5)).unwrap();
        step(&mut ed); // 2: edit a
        ed.select_only(Some(b));
        step(&mut ed); // 3: select b

        ed.undo(); // undoes the selection of b only
        assert_eq!(ed.selected, Some(a));
        assert_eq!(transparency(&ed, a), 0.5, "the edit is still there");
        ed.undo(); // undoes the edit only
        assert_eq!(transparency(&ed, a), 0.0);
        assert_eq!(ed.selected, Some(a), "and the selection is left alone");
        assert!(ed.dirty);
        ed.undo();
        assert!(ed.selected.is_none());
        ed.redo();
        ed.redo();
        assert_eq!(transparency(&ed, a), 0.5, "redo replays them in order");
        assert_eq!(ed.selected, Some(a));
        ed.redo();
        assert_eq!(ed.selected, Some(b));
    }

    #[test]
    fn an_edit_that_also_changes_the_selection_undoes_both_together() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let a = wall(&ed);
        let count = |ed: &Editor| ed.dm.borrow().subtree(ed.dm.borrow().workspace).len();
        ed.select_only(Some(a));
        step(&mut ed);
        let before = count(&ed);

        ed.run_action(crate::keymap::Action::Copy);
        ed.run_action(crate::keymap::Action::Paste); // selects what it pasted
        step(&mut ed);
        let pasted = ed.selected.unwrap();
        assert_ne!(pasted, a);
        assert_eq!(count(&ed), before + 1);

        ed.undo();
        assert_eq!(count(&ed), before, "the paste is gone");
        assert_eq!((ed.selection.clone(), ed.selected), (vec![a], Some(a)), "and the earlier selection is back, in one step");
        ed.redo();
        assert_eq!(count(&ed), before + 1);
        assert!(ed.selected.is_some_and(|id| ed.dm.borrow().exists(id) && id != a), "the pasted copy is selected again (the revived object, under its new id)");
    }

    #[test]
    fn deleting_the_selection_and_undoing_it_restores_the_selection() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let ids: Vec<Id> = ["Wall", "Step 1"].iter().map(|n| ed.dm.borrow().find_first_child(ed.dm.borrow().workspace, n, true).unwrap()).collect();
        ed.select_only(Some(ids[0]));
        ed.select_toggle(ids[1]);
        step(&mut ed);
        ed.delete_selected();
        step(&mut ed);
        assert!(ed.selection.is_empty());
        ed.undo();
        let names: Vec<String> = ed.selection.iter().map(|id| ed.dm.borrow().name(*id)).collect();
        assert_eq!(names, ["Wall", "Step 1"], "both come back selected, active one last");
        assert_eq!(ed.selected.map(|id| ed.dm.borrow().name(id)).as_deref(), Some("Step 1"));
    }

    #[test]
    fn clicking_what_is_already_selected_is_not_a_step_and_a_new_selection_ends_redo() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (wall(&ed), ed.dm.borrow().find_first_child(ed.dm.borrow().workspace, "Step 1", false).unwrap());
        ed.select_only(Some(a));
        step(&mut ed);
        ed.select_only(Some(a));
        step(&mut ed);
        ed.undo();
        assert!(ed.selection.is_empty(), "the repeated click recorded nothing: one undo empties the selection");

        ed.redo();
        ed.undo();
        ed.select_only(Some(b));
        step(&mut ed);
        assert!(!ed.history.can_redo(), "selecting something new ends the chance to redo");
    }

    #[test]
    fn a_click_that_becomes_a_drag_is_two_steps_not_one() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let a = wall(&ed);
        let start = ed.dm.borrow().vec3(a, "Position");
        // The press selects (the mouse stays down: a gesture) ...
        ed.select_only(Some(a));
        ed.commit_edits(true);
        // ... then the drag moves it, and the release ends the gesture.
        ed.dm.borrow_mut().set_prop(a, "Position", Value::V3(start + bevy::math::Vec3::X * 4.0)).unwrap();
        ed.commit_edits(true);
        ed.commit_edits(false);

        ed.undo();
        assert_eq!(ed.dm.borrow().vec3(a, "Position"), start, "the first undo takes back the move");
        assert_eq!(ed.selected, Some(a), "and keeps the selection");
        ed.undo();
        assert!(ed.selected.is_none(), "the second takes back the selection");
    }

    #[test]
    fn selection_history_works_across_a_play_session() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (wall(&ed), ed.dm.borrow().find_first_child(ed.dm.borrow().workspace, "Step 1", false).unwrap());
        ed.select_only(Some(a));
        step(&mut ed);
        ed.select_only(Some(b));
        step(&mut ed);

        ed.start_play();
        ed.select_only(None); // selecting while playing is not recorded
        ed.commit_edits(false);
        ed.stop_play();
        assert_eq!(ed.selected.map(|id| ed.dm.borrow().name(id)).as_deref(), Some("Step 1"), "the selection from before Play is back");

        ed.undo();
        assert_eq!(ed.selected.map(|id| ed.dm.borrow().name(id)).as_deref(), Some("Wall"), "history reaches the rebuilt objects");
        ed.undo();
        assert!(ed.selected.is_none());
    }
}

impl Editor {
    /// The whole scene as a file (what Play restores on Stop).
    pub(super) fn snapshot(&self) -> PlaceFile {
        self.dm.borrow().to_place(&self.scene_name)
    }

    /// Called once per frame. Turns the edits recorded in the data model (and a changed selection) into
    /// an undo step, unless the mouse is still held (a drag or a slider is one gesture, however many
    /// frames it takes). A selection change is not held back by that: clicking an object is a step of its
    /// own even if the click turns into a drag; only when edits are already waiting does it join them.
    pub fn commit_edits(&mut self, gesture_in_progress: bool) {
        if self.play.is_some() {
            return;
        }
        if !gesture_in_progress {
            return self.flush_edits();
        }
        if !self.dm.borrow().journal_pending() {
            self.record_selection_step();
        }
    }

    /// Makes whatever has been edited or selected so far an undo step right now.
    pub(super) fn flush_edits(&mut self) {
        let changes = self.dm.borrow_mut().take_journal();
        let now = self.selection_state();
        let selection = (now != self.sel_committed).then(|| (self.sel_committed.clone(), now.clone()));
        self.sel_committed = now;
        self.history.record(changes, selection, Instant::now());
    }

    /// Records a selection that changed on its own.
    fn record_selection_step(&mut self) {
        let now = self.selection_state();
        if now != self.sel_committed {
            let before = std::mem::replace(&mut self.sel_committed, now.clone());
            self.history.record(Vec::new(), Some((before, now)), Instant::now());
        }
    }

    pub fn undo(&mut self) {
        if self.play.is_some() {
            return self.warn("Undo is off while the game is playing: its changes are discarded when it stops.");
        }
        self.forget_duplicate(); // (what Repeat Duplicate would repeat may be the very thing undone)
        self.flush_edits();
        let dm = self.dm.clone();
        let applied = self.history.undo(&mut dm.borrow_mut());
        self.after_replay(applied);
    }

    pub fn redo(&mut self) {
        if self.play.is_some() {
            return;
        }
        self.forget_duplicate();
        self.flush_edits();
        let dm = self.dm.clone();
        let applied = self.history.redo(&mut dm.borrow_mut());
        self.after_replay(applied);
    }

    fn after_replay(&mut self, applied: Option<Applied>) {
        let Some(applied) = applied else { return };
        for p in applied.problems {
            self.warn(format!("undo: {p}"));
        }
        // Put back the whole selection the step recorded: the same objects, the same active one.
        if let Some(state) = applied.selection {
            self.apply_selection_state(&state);
        }
        self.normalize_selection();
        if applied.focus.is_some_and(|f| self.is_selected(f)) {
            self.reveal = true;
        }
        // What was just put back is not a new change to record.
        self.sel_committed = self.selection_state();
        if applied.edited {
            self.dirty = true;
        }
    }
}