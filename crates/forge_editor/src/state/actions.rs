//! What each shortcut does. The keyboard side (which chord means which action, and in which window)
//! is in `keymap`; this is the other half: running an action.

use forge_engine::{
    core::Value,
    dm::{DataModel, Id},
};

use super::{Editor, Increment, Tool};
use crate::{
    keymap::{self, Action},
    tools,
};

impl Editor {
    /// Runs the shortcuts that were pressed this frame.
    pub fn run_actions(&mut self, frame: &keymap::Frame) {
        for action in &frame.pressed {
            self.run_action(*action);
        }
    }

    /// Runs one shortcut. (Held shortcuts and the mouse wheel are read where they are used: the
    /// viewport camera.)
    pub fn run_action(&mut self, action: Action) {
        // While a game has the keyboard (Game camera, viewport focused) only the shortcuts that
        // leave the game alone get through; those are the ones that work from inside text fields.
        // (The debug overlays are the exception: they leave the game alone, and are most useful while it runs.)
        if self.game_has_input() && !action.def().while_typing && !matches!(action, Action::Debug(_)) {
            return;
        }
        match action {
            Action::Debug(overlay) => self.toggle_overlay(overlay),
            Action::Save => {
                self.dirty = true;
                self.save_all();
            }
            Action::Duplicate => self.duplicate_selected(),
            Action::RepeatDuplicate => self.repeat_duplicate(),
            Action::Delete => self.delete_selected(),
            Action::Copy => self.copy_selected(),
            Action::Cut => {
                self.copy_selected();
                self.delete_selected();
            }
            Action::Paste => self.paste(),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Play => self.toggle_play(),
            Action::Stop => self.stop_play(),
            // (The settings windows: the same shortcut opens and closes them. They are windows of their own, not panels.)
            Action::EditorSettings => self.toggle_editor_settings(),
            Action::ProjectSettings => self.toggle_project_settings(),
            Action::Window(panel) => self.toggle_panel(panel),
            Action::RestoreLayout => self.restore_layout(),

            Action::SnapTo => self.focus_request = true,
            Action::InsertObject => {
                if self.project.is_some() {
                    self.quick_insert = Some(String::new());
                }
            }
            Action::Wireframe => {
                self.settings.render.wireframe = !self.settings.render.wireframe;
                self.settings.touch();
            }
            Action::WireframeOnly => {
                self.settings.render.wireframe_only = !self.settings.render.wireframe_only;
                self.settings.touch();
            }
            Action::ToolSelect => self.tool = Tool::Select,
            Action::ToolMove => self.tool = Tool::Move,
            Action::ToolScale => self.tool = Tool::Scale,
            Action::ToolRotate => self.tool = Tool::Rotate,
            Action::ToolPivot => self.tool = Tool::Pivot,
            Action::TransformSpace => {
                self.settings.space = self.settings.space.toggled();
                self.settings.touch();
            }
            Action::Rotate90Y => self.rotate_selection(1),
            Action::Rotate90X => self.rotate_selection(0),
            Action::MoveIncrement => self.increment_popup = Some(Increment::Move),
            Action::RotateIncrement => self.increment_popup = Some(Increment::Rotate),

            Action::Group => self.wrap_selected("Model"),
            Action::Folder => self.wrap_selected("Folder"),
            Action::Ungroup => self.ungroup_selected(),
            Action::FindSelected => {
                self.tree_filter.reset();
                self.reveal = true;
            }
            Action::Rename => self.begin_rename(),

            // Held / wheel shortcuts: handled by the viewport camera.
            Action::CamForward
            | Action::CamLeft
            | Action::CamBack
            | Action::CamRight
            | Action::CamUp
            | Action::CamDown
            | Action::FastCamera
            | Action::FastSpeedWheel
            | Action::PrimarySpeedWheel
            | Action::ZoomWheel => {}
        }
    }

    /// Switches a debug overlay on or off: the same switch as its checkbox in the Debug menu.
    pub fn toggle_overlay(&mut self, overlay: forge_engine::diag::Overlay) {
        self.settings.debug.toggle(overlay);
        self.settings.touch();
    }

    // ---- clipboard ------------------------------------------------------------------------------

    fn copy_selected(&mut self) {
        self.normalize_selection();
        let dm = self.dm.borrow();
        let copies: Vec<_> = self.top_level_selection().into_iter().filter(|id| dm.get(*id).is_some_and(|i| i.class.desc.service_order.is_none())).map(|id| dm.capture_subtree(id)).collect();
        if copies.is_empty() {
            return;
        }
        let count = copies.len();
        self.clipboard = copies;
        drop(dm);
        self.info(if count == 1 { "Copied 1 object.".to_owned() } else { format!("Copied {count} objects.") });
    }

    fn paste(&mut self) {
        if self.clipboard.is_empty() || self.project.is_none() {
            return;
        }
        self.normalize_selection();
        // Into the active object if it is a container, else into the workspace.
        let parent = {
            let dm = self.dm.borrow();
            self.selected.filter(|s| dm.get(*s).is_some_and(|i| i.class.desc.container)).unwrap_or(dm.workspace)
        };
        let mut pasted = Vec::new();
        for copy in self.clipboard.clone() {
            let result = self.dm.borrow_mut().paste_subtree(&copy, parent);
            match result {
                Ok(id) => pasted.push(id),
                Err(e) => self.warn(format!("could not paste: {e}")),
            }
        }
        if let Some(last) = pasted.last().copied() {
            self.selection = pasted;
            self.selected = Some(last);
            self.last_inspected = Some(last);
            self.reveal = true;
            self.dirty = true;
        }
    }

    // ---- transforming ---------------------------------------------------------------------------

    /// Turns the selection a quarter turn around the X (`axis` 0) or Y (`axis` 1) axis, as ONE group: every
    /// selected object swings round the same point, so how they sit relative to each other is kept.
    /// The point and the axes are the Rotate tool's own (see `tools::frame_for`): the pivot of a single
    /// object, or the middle of everything selected; the world's axes, or in Local space the active
    /// object's. Objects inside a selected object go with it (they are not turned a second time).
    ///
    /// While an object is being dragged in the viewport the drag owns the transforms (it measures every frame from
    /// where the objects were when it began), so the turn is left for the viewport to hand to the drag
    /// (`pending_turns`, see `tools::quarter_turn`); turning the objects here would be undone by its next frame.
    fn rotate_selection(&mut self, axis: usize) {
        if self.drag_active {
            self.pending_turns.push(axis);
            return;
        }
        self.turn_selection(axis, None);
    }

    /// Does the quarter turn now, on the selection (taken into `drag` when one is given).
    pub fn turn_selection(&mut self, axis: usize, drag: Option<&mut tools::Drag>) {
        self.normalize_selection();
        let targets = self.top_level_selection();
        let turned = tools::quarter_turn(&mut self.dm.borrow_mut(), drag, self.settings.space, &targets, self.selected, axis);
        if turned {
            self.dirty = true;
        }
    }

    // ---- the scene tree -------------------------------------------------------------------------

    /// Puts the selected objects inside one new `Model` or `Folder` that takes the place of the first of them.
    fn wrap_selected(&mut self, class: &str) {
        self.normalize_selection();
        let targets: Vec<Id> = {
            let dm = self.dm.borrow();
            self.top_level_selection().into_iter().filter(|id| Self::can_move(&dm, *id)).collect()
        };
        let Some(first) = targets.first().copied() else { return };
        let active = self.selected;
        let mut dm = self.dm.borrow_mut();
        let Some((parent, index)) = dm.slot_in_parent(first) else { return };
        let container = match dm.create(class) {
            Ok(c) => c,
            Err(e) => {
                drop(dm);
                return self.warn(e);
            }
        };
        let mut moved = dm.set_parent_at(container, parent, index);
        for id in &targets {
            if moved.is_ok() {
                moved = dm.set_parent(*id, Some(container));
            }
        }
        if let Err(e) = moved {
            dm.destroy(container);
            drop(dm);
            return self.warn(e);
        }
        // A model made around a part is "about" that part.
        let primary = active.filter(|a| targets.contains(a)).unwrap_or(first);
        if class == "Model" && dm.is_a(primary, "BasePart") {
            let _ = dm.set_prop(container, "PrimaryPart", Value::Ref(Some(primary)));
        }
        drop(dm);
        self.select_only(Some(container));
        self.reveal = true;
        self.dirty = true;
    }

    /// Dissolves every selected `Model` / `Folder`: their children take their place and they are deleted.
    fn ungroup_selected(&mut self) {
        self.normalize_selection();
        let mut freed: Vec<Id> = Vec::new();
        let groups: Vec<Id> = {
            let dm = self.dm.borrow();
            self.top_level_selection().into_iter().filter(|id| dm.get(*id).is_some_and(|i| matches!(i.class.name, "Model" | "Folder")) && Self::can_move(&dm, *id)).collect()
        };
        let mut dm = self.dm.borrow_mut();
        for group in groups {
            let Some((parent, index)) = dm.slot_in_parent(group) else { continue };
            let children = dm.children(group);
            let mut failed = None;
            for (i, child) in children.iter().enumerate() {
                if let Err(e) = dm.set_parent_at(*child, parent, index + i) {
                    failed = Some(e);
                    break;
                }
            }
            if let Some(e) = failed {
                drop(dm);
                return self.warn(e);
            }
            dm.destroy(group);
            freed.extend(children);
        }
        drop(dm);
        if let Some(last) = freed.last().copied() {
            self.selection = freed;
            self.selected = Some(last);
            self.last_inspected = Some(last);
            self.reveal = true;
            self.dirty = true;
        }
    }

    /// Services (and the root) cannot be moved, wrapped or renamed.
    fn can_move(dm: &DataModel, id: Id) -> bool {
        dm.get(id).is_some_and(|i| i.class.desc.service_order.is_none() && !i.class.desc.root)
    }

    fn begin_rename(&mut self) {
        let Some(sel) = self.selected else { return };
        let dm = self.dm.borrow();
        if !Self::can_move(&dm, sel) {
            return;
        }
        let name = dm.name(sel);
        drop(dm);
        self.renaming = Some((sel, name));
        self.reveal = true;
    }

    /// Finishes (or abandons) the rename started with `begin_rename`.
    pub fn finish_rename(&mut self, accept: bool) {
        let Some((id, name)) = self.renaming.take() else { return };
        let name = name.trim().to_owned();
        if accept && !name.is_empty() {
            let old = self.dm.borrow().name(id);
            let result = self.dm.borrow_mut().set_prop(id, "Name", Value::Str(name));
            match result {
                Ok(()) => {
                    self.dirty = true;
                    self.queue_script_rename(id, &old);
                }
                Err(e) => self.warn(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{Quat, Vec3};
    use forge_engine::project::Project;

    use super::*;
    use crate::dock::Panel;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_actions_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Actions Test").expect("create project");
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
    fn group_wraps_the_selection_in_place_and_undo_unwraps_it() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let step = find(&ed, "Step 2");
        let (ws, slot) = {
            let dm = ed.dm.borrow();
            dm.slot_in_parent(step).unwrap()
        };
        ed.selected = Some(step);

        ed.run_action(Action::Group);
        ed.commit_edits(false);
        let model = ed.selected.expect("the new model is selected");
        assert_eq!(ed.dm.borrow().class_name(model), "Model");
        assert_eq!(ed.dm.borrow().parent(step), Some(model));
        assert_eq!(ed.dm.borrow().slot_in_parent(model), Some((ws, slot)), "the model takes the part's place in the tree");
        assert_eq!(ed.dm.borrow().reference(model, "PrimaryPart"), Some(step));

        ed.undo();
        assert_eq!(ed.dm.borrow().slot_in_parent(step), Some((ws, slot)));
        assert!(!ed.dm.borrow().exists(model));
    }

    #[test]
    fn folder_and_ungroup_are_inverses() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = find(&ed, "Wall");
        let before = ed.dm.borrow().slot_in_parent(wall).unwrap();
        ed.selected = Some(wall);
        ed.run_action(Action::Folder);
        let folder = ed.selected.unwrap();
        assert_eq!(ed.dm.borrow().class_name(folder), "Folder");

        ed.run_action(Action::Ungroup);
        assert!(!ed.dm.borrow().exists(folder), "the folder is gone");
        assert_eq!(ed.dm.borrow().slot_in_parent(wall), Some(before), "the part is back where it was");
        assert_eq!(ed.selected, Some(wall));
        // Ungroup does nothing to something that is not a group.
        ed.run_action(Action::Ungroup);
        assert!(ed.dm.borrow().exists(wall));
    }

    #[test]
    fn copy_paste_and_cut_make_independent_copies() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = find(&ed, "Wall");
        let count = |ed: &Editor| ed.dm.borrow().subtree(ed.dm.borrow().workspace).iter().filter(|i| ed.dm.borrow().name(**i) == "Wall").count();
        ed.selected = Some(wall);

        ed.run_action(Action::Copy);
        ed.run_action(Action::Paste);
        assert_eq!(count(&ed), 2);
        let pasted = ed.selected.unwrap();
        assert_ne!(pasted, wall);
        assert_eq!(ed.dm.borrow().size(pasted), ed.dm.borrow().size(wall), "the copy has the same properties");

        ed.commit_edits(false);
        ed.undo();
        assert_eq!(count(&ed), 1, "a paste is one undo step");

        ed.selected = Some(wall);
        ed.run_action(Action::Cut);
        assert_eq!(count(&ed), 0);
        ed.run_action(Action::Paste);
        assert_eq!(count(&ed), 1, "a cut part can be pasted back");
    }

    #[test]
    fn rotating_turns_the_selection_a_quarter_in_world_or_local_space() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = find(&ed, "Wall");
        ed.selected = Some(wall);
        let forward = |ed: &Editor| ed.dm.borrow().get_pivot(wall).map(|(_, q)| q * Vec3::NEG_Z).unwrap();

        ed.run_action(Action::Rotate90Y);
        assert!((forward(&ed) - Vec3::NEG_X).length() < 1e-4, "a quarter turn around Y");
        ed.run_action(Action::Rotate90X);
        assert!(forward(&ed).abs_diff_eq(Vec3::NEG_X, 1e-4), "a world X turn does not move the X axis");

        ed.run_action(Action::TransformSpace);
        assert_eq!(ed.settings.space, crate::settings::Space::Local);

        assert!(!ed.settings.render.wireframe);
        ed.run_action(Action::Wireframe);
        assert!(ed.settings.render.wireframe, "the shortcut turns the wireframe view on");
        ed.run_action(Action::Wireframe);
        assert!(!ed.settings.render.wireframe, "and off again");

        assert!(!ed.settings.render.wireframe_only);
        ed.run_action(Action::WireframeOnly);
        assert!(ed.settings.render.wireframe_only, "and the solids can be hidden");
    }

    fn pose(ed: &Editor, id: Id) -> (Vec3, Quat) {
        ed.dm.borrow().get_pivot(id).unwrap()
    }

    /// The middle of the boxes of these objects: the point a group turns about.
    fn middle(ed: &Editor, ids: &[Id]) -> Vec3 {
        let dm = ed.dm.borrow();
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for id in ids {
            let (a, b) = dm.collider(*id).unwrap().aabb();
            min = min.min(a);
            max = max.max(b);
        }
        (min + max) * 0.5
    }

    #[test]
    fn rotating_several_objects_turns_them_as_one_group_about_their_middle() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b, c) = (find(&ed, "Step 1"), find(&ed, "Step 2"), find(&ed, "Step 3"));
        let neighbour = find(&ed, "Wall");
        let neighbour_before = pose(&ed, neighbour);
        let before: Vec<(Vec3, Quat)> = [a, b, c].iter().map(|id| pose(&ed, *id)).collect();
        let centre = middle(&ed, &[a, b, c]);
        ed.select_only(Some(a));
        ed.select_toggle(b);
        ed.select_toggle(c);

        ed.run_action(Action::Rotate90Y);
        let turn = Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
        for ((id, (p, q)), name) in [a, b, c].iter().zip(&before).zip(["Step 1", "Step 2", "Step 3"]) {
            let (now_p, now_q) = pose(&ed, *id);
            let want = centre + turn * (*p - centre);
            assert!(now_p.abs_diff_eq(want, 1e-3), "{name} swung round the middle of the group: {now_p} vs {want}");
            assert!(now_q.angle_between((turn * *q).normalize()) < 1e-4, "{name} turned with it");
        }
        // How they sit relative to each other is kept (a rigid turn of the whole group).
        let d = |ed: &Editor, x: Id, y: Id| (pose(ed, x).0 - pose(ed, y).0).length();
        assert!((d(&ed, a, b) - (before[0].0 - before[1].0).length()).abs() < 1e-3 && (d(&ed, b, c) - (before[1].0 - before[2].0).length()).abs() < 1e-3);
        // Nobody else moved, and nothing changed parents.
        assert_eq!(pose(&ed, neighbour), neighbour_before);
        assert_eq!(ed.dm.borrow().parent(a), Some(ed.dm.borrow().workspace));

        // It is one step to undo, and four quarter turns come back to the start (no object turned twice per press).
        ed.commit_edits(false);
        ed.undo();
        for (id, (p, q)) in [a, b, c].iter().zip(&before) {
            let (now_p, now_q) = pose(&ed, *id);
            assert!(now_p.abs_diff_eq(*p, 1e-3) && now_q.angle_between(*q) < 1e-4, "undo restored all three together");
        }
        ed.redo();
        assert!(pose(&ed, a).0.abs_diff_eq(centre + turn * (before[0].0 - centre), 1e-3), "and redo does it again");
        for _ in 0..3 {
            ed.run_action(Action::Rotate90Y);
        }
        for (id, (p, _)) in [a, b, c].iter().zip(&before) {
            assert!(pose(&ed, *id).0.abs_diff_eq(*p, 1e-2));
        }
    }

    #[test]
    fn a_group_turn_takes_nested_objects_along_once_and_uses_the_active_objects_axes_in_local_space() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = find(&ed, "Player");
        let root = ed.dm.borrow().find_child(model, "Body").unwrap();
        let wall = find(&ed, "Wall");
        let root_before = pose(&ed, root);
        let wall_before = pose(&ed, wall);
        // The model and its own part are both selected: only the model is turned, the part rides along.
        ed.select_only(Some(model));
        ed.select_toggle(root);
        ed.select_toggle(wall);
        let centre = middle(&ed, &[model, wall].iter().flat_map(|m| ed.dm.borrow().subtree(*m)).filter(|n| ed.dm.borrow().collider(*n).is_some()).collect::<Vec<_>>());
        ed.run_action(Action::Rotate90Y);
        let turn = Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
        assert!(pose(&ed, root).0.abs_diff_eq(centre + turn * (root_before.0 - centre), 1e-3), "the part went round once, with its model");
        assert!(pose(&ed, wall).0.abs_diff_eq(centre + turn * (wall_before.0 - centre), 1e-3));
        assert_eq!(ed.dm.borrow().parent(root), Some(model));

        // Local space: the turn is about the active object's own X axis, still for the group.
        let (p, q) = (find(&ed, "Step 1"), find(&ed, "Step 2"));
        ed.dm.borrow_mut().set_prop(p, "Orientation", forge_engine::core::Value::V3(Vec3::new(0.0, 45.0, 0.0))).unwrap();
        ed.run_action(Action::TransformSpace);
        ed.select_only(Some(q));
        ed.select_toggle(p);
        assert_eq!(ed.selected, Some(p));
        let (before_p, before_q) = (pose(&ed, p), pose(&ed, q));
        let centre = middle(&ed, &[p, q]);
        let axis = (before_p.1 * Vec3::X).normalize();
        let turn = Quat::from_axis_angle(axis, std::f32::consts::FRAC_PI_2);
        ed.run_action(Action::Rotate90X);
        assert!(pose(&ed, p).0.abs_diff_eq(centre + turn * (before_p.0 - centre), 1e-3) && pose(&ed, q).0.abs_diff_eq(centre + turn * (before_q.0 - centre), 1e-3));
        assert!(pose(&ed, q).1.angle_between((turn * before_q.1).normalize()) < 1e-4);
    }

    #[test]
    fn rename_changes_the_name_and_cancel_keeps_it() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = find(&ed, "Wall");
        ed.selected = Some(wall);
        ed.run_action(Action::Rename);
        assert_eq!(ed.renaming.as_ref().map(|r| r.1.as_str()), Some("Wall"));
        ed.renaming.as_mut().unwrap().1 = "Fence".into();
        ed.finish_rename(true);
        assert_eq!(ed.dm.borrow().name(wall), "Fence");

        ed.run_action(Action::Rename);
        ed.renaming.as_mut().unwrap().1 = "Nope".into();
        ed.finish_rename(false);
        assert_eq!(ed.dm.borrow().name(wall), "Fence");
        ed.commit_edits(false);
        ed.undo();
        assert_eq!(ed.dm.borrow().name(wall), "Wall", "a rename is undoable");
    }

    #[test]
    fn services_cannot_be_grouped_renamed_or_ungrouped() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        ed.selected = Some(ed.dm.borrow().workspace);
        ed.run_action(Action::Group);
        ed.run_action(Action::Rename);
        ed.run_action(Action::Ungroup);
        assert!(ed.renaming.is_none());
        assert_eq!(ed.selected, Some(ed.dm.borrow().workspace));
    }

    #[test]
    fn tool_and_window_shortcuts_set_state() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        ed.run_action(Action::ToolRotate);
        assert_eq!(ed.tool, Tool::Rotate);
        ed.run_action(Action::MoveIncrement);
        assert_eq!(ed.increment_popup, Some(Increment::Move));
        ed.run_action(Action::InsertObject);
        assert!(ed.quick_insert.is_some());
        ed.run_action(Action::EditorSettings);
        assert!(ed.editor_settings_open());
        ed.run_action(Action::EditorSettings);
        assert!(!ed.editor_settings_open(), "the shortcut toggles the window");
    }

    #[test]
    fn the_window_shortcuts_show_focus_and_hide_panels_and_restore_the_layout() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        assert!(ed.panel_visible(Panel::Explorer));
        // The Explorer is shown but not the one in use: the first press brings it forward, the second hides it.
        ed.run_action(Action::Window(Panel::Explorer));
        assert!(ed.panel_visible(Panel::Explorer) && ed.dock.is_focused(Panel::Explorer));
        ed.run_action(Action::Window(Panel::Explorer));
        assert!(!ed.panel_visible(Panel::Explorer));
        ed.run_action(Action::Window(Panel::Explorer));
        assert!(ed.panel_visible(Panel::Explorer) && ed.dock.is_focused(Panel::Explorer), "the third shows it again");
        ed.run_action(Action::Window(Panel::Output));
        ed.run_action(Action::Window(Panel::Output));
        assert!(!ed.panel_visible(Panel::Output));
        ed.run_action(Action::RestoreLayout);
        assert_eq!(ed.dock.to_json(), crate::dock::Dock::default_layout().to_json());
        assert_eq!(Action::Window(Panel::Assets).def().defaults, ["Alt+5"]);
        assert!(!Action::Window(Panel::Assets).def().while_typing, "a window shortcut never fires while typing");
        assert_eq!(Action::Window(Panel::Assets).def().context, crate::keymap::Context::Global);
    }

    // ---- repeating the last duplicate ----

    fn count_named(ed: &Editor, name: &str) -> usize {
        let dm = ed.dm.borrow();
        dm.subtree(dm.workspace).into_iter().filter(|i| dm.name(*i) == name).count()
    }

    /// Moves the selected object the way the gizmo would: through its position.
    fn nudge(ed: &Editor, id: Id, by: Vec3) {
        let p = ed.dm.borrow().vec3(id, "Position");
        ed.dm.borrow_mut().set_prop(id, "Position", Value::V3(p + by)).unwrap();
    }

    #[test]
    fn repeating_a_duplicate_makes_evenly_spaced_copies_and_each_one_undoes_on_its_own() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = find(&ed, "Wall");
        let start = ed.dm.borrow().vec3(wall, "Position");
        ed.select_only(Some(wall));

        ed.run_action(Action::Duplicate);
        ed.commit_edits(false);
        let first = ed.selected.unwrap();
        assert_ne!(first, wall);
        assert_eq!(ed.dm.borrow().vec3(first, "Position"), start, "a duplicate lands on its original");
        nudge(&ed, first, Vec3::new(0.0, 0.0, 12.0));
        ed.commit_edits(false);

        let mut copies = vec![first];
        for n in 2..=4 {
            ed.run_action(Action::RepeatDuplicate);
            ed.commit_edits(false);
            let latest = ed.selected.unwrap();
            assert!(!copies.contains(&latest) && latest != wall, "each repeat makes a new object");
            assert_eq!(ed.selection, vec![latest], "the new copy is the selection, as after Duplicate");
            assert_eq!(ed.dm.borrow().vec3(latest, "Position"), start + Vec3::new(0.0, 0.0, 12.0 * n as f32), "copy {n} is one offset further than the last");
            copies.push(latest);
        }
        assert_eq!(count_named(&ed, "Wall"), 5, "the original and every copy are still there");
        assert_eq!(ed.dm.borrow().vec3(wall, "Position"), start, "the original never moved");
        for (i, c) in copies.iter().enumerate() {
            assert_eq!(ed.dm.borrow().vec3(*c, "Position").z, start.z + 12.0 * (i + 1) as f32, "earlier copies stay where they were put");
        }

        // One press, one undo step (the history sees the repeat, not the move and the copy separately).
        ed.undo();
        assert_eq!(count_named(&ed, "Wall"), 4, "the last repeat is gone and only it");
        assert!(ed.last_duplicate.is_none(), "undo forgets what to repeat: the copies it knew may be the ones that went");
        ed.redo();
        assert_eq!(count_named(&ed, "Wall"), 5, "and redo brings it back");
    }

    #[test]
    fn a_repeated_duplicate_repeats_the_turn_as_well_as_the_move() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = find(&ed, "Wall");
        ed.select_only(Some(wall));
        ed.run_action(Action::Duplicate);
        let first = ed.selected.unwrap();
        // Moved and turned a quarter: the next copy is moved and turned again, a half turn from the original.
        let (p0, q0) = pose(&ed, first);
        let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        ed.dm.borrow_mut().pivot_to(first, (p0 + Vec3::new(8.0, 0.0, 0.0), (turn * q0).normalize()));
        let (p1, q1) = pose(&ed, first);

        ed.run_action(Action::RepeatDuplicate);
        let second = ed.selected.unwrap();
        let (p2, q2) = pose(&ed, second);
        let shift = p1 - turn * p0;
        assert!(p2.abs_diff_eq(turn * p1 + shift, 1e-3), "the same move and turn, applied to the copy: {p2}");
        assert!(q2.angle_between((turn * q1).normalize()) < 1e-4, "a half turn from the original");
        ed.run_action(Action::RepeatDuplicate);
        let (p3, q3) = pose(&ed, ed.selected.unwrap());
        assert!(p3.abs_diff_eq(turn * p2 + shift, 1e-3) && q3.angle_between((turn * q2).normalize()) < 1e-4, "and again from there");
        let _ = wall;
    }

    #[test]
    fn repeating_a_duplicate_of_several_objects_repeats_each_by_its_own_offset() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (find(&ed, "Step 1"), find(&ed, "Step 2"));
        let (pa, pb) = (ed.dm.borrow().vec3(a, "Position"), ed.dm.borrow().vec3(b, "Position"));
        ed.select_only(Some(a));
        ed.select_toggle(b);
        ed.run_action(Action::Duplicate);
        let copies = ed.selection.clone();
        assert_eq!(copies.len(), 2);
        let by = Vec3::new(0.0, 0.0, 9.0);
        for c in &copies {
            nudge(&ed, *c, by);
        }
        ed.run_action(Action::RepeatDuplicate);
        assert_eq!(ed.selection.len(), 2, "both copies are repeated and both new ones are selected");
        assert!(ed.selection.iter().all(|n| !copies.contains(n)));
        let mut now: Vec<Vec3> = ed.selection.iter().map(|n| ed.dm.borrow().vec3(*n, "Position")).collect();
        let mut want = vec![pa + by * 2.0, pb + by * 2.0];
        let key = |v: &Vec3| (v.x * 100.0) as i64;
        now.sort_by_key(key);
        want.sort_by_key(key);
        assert!(now.iter().zip(&want).all(|(x, y)| x.abs_diff_eq(*y, 1e-3)), "{now:?} vs {want:?}");
        assert_eq!(count_named(&ed, "Step 1"), 3);
        assert_eq!(count_named(&ed, "Step 2"), 3);
    }

    #[test]
    fn repeat_duplicate_is_reset_by_what_makes_it_meaningless_and_says_so() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (wall, step) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        // Nothing duplicated yet: nothing happens (and it is said).
        ed.select_only(Some(wall));
        ed.run_action(Action::RepeatDuplicate);
        assert_eq!(count_named(&ed, "Wall"), 1);
        assert!(ed.log.last().unwrap().text.contains("Nothing to repeat"));

        // A different selection: the record is dropped, not applied to what is selected now.
        ed.run_action(Action::Duplicate);
        assert!(ed.last_duplicate.is_some());
        ed.select_only(Some(step));
        ed.run_action(Action::RepeatDuplicate);
        assert!(ed.last_duplicate.is_none() && count_named(&ed, "Wall") == 2 && count_named(&ed, "Step 1") == 1, "nothing was copied");
        assert!(ed.log.last().unwrap().text.contains("reset"));

        // The copies gone (deleted): reset. Opening Play and stopping it: reset.
        ed.select_only(Some(wall));
        ed.run_action(Action::Duplicate);
        let copy = ed.selected.unwrap();
        ed.dm.borrow_mut().destroy(copy);
        ed.select_only(Some(wall));
        ed.run_action(Action::RepeatDuplicate);
        assert!(ed.last_duplicate.is_none());
        ed.run_action(Action::Duplicate);
        assert!(ed.last_duplicate.is_some());
        ed.start_play();
        assert!(ed.last_duplicate.is_none(), "a game running is not an edit");
        ed.stop_play();
        // Ctrl+D itself is untouched by all this: it always duplicates, and records.
        let before = count_named(&ed, "Wall");
        ed.run_action(Action::Duplicate);
        assert_eq!(count_named(&ed, "Wall"), before + 1);
        assert!(ed.last_duplicate.is_some());
    }

    // ---- quarter turns during a drag, and undo ----

    #[test]
    fn a_shortcut_turn_while_dragging_waits_for_the_drag_and_is_one_step_with_it_for_undo() {
        use crate::{
            gizmo::MoveHandle,
            tools::{begin_drag, frame_for, update_drag, Handle, Snap},
        };
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = find(&ed, "Wall");
        ed.select_only(Some(wall));
        let start = pose(&ed, wall);

        // A move drag in progress (what the viewport owns): the shortcut is queued for it, not applied behind its back.
        let cam = Vec3::new(-30.0, 20.0, 60.0);
        let f = frame_for(&ed.dm.borrow(), Tool::Move, ed.settings.space, wall, &[wall], cam).unwrap();
        let grab = f.origin + Vec3::X * (f.len * 0.5);
        let r = (cam, (grab - cam).normalize());
        let mut drag = begin_drag(&ed.dm.borrow(), Handle::Move(MoveHandle::Arrow(0, 1.0)), f, r, &[wall], wall).expect("the arrow was grabbed");
        let free = Snap::steps(0.0, 0.0);
        ed.drag_active = true;
        update_drag(&mut ed.dm.borrow_mut(), &mut drag, (cam, (grab + Vec3::X * 6.0 - cam).normalize()), &free);
        let moved = pose(&ed, wall);
        ed.run_action(Action::Rotate90X);
        assert_eq!(ed.pending_turns, vec![0], "queued for the drag");
        assert_eq!(pose(&ed, wall).1, moved.1, "and nothing was turned behind its back");

        // The viewport hands it to the drag in its next frame.
        for axis in std::mem::take(&mut ed.pending_turns) {
            ed.turn_selection(axis, Some(&mut drag));
        }
        let turned = pose(&ed, wall);
        assert!(turned.1.angle_between(moved.1) > 1.5 && turned.0.abs_diff_eq(moved.0, 1e-3));
        update_drag(&mut ed.dm.borrow_mut(), &mut drag, (cam, (grab + Vec3::X * 6.0 - cam).normalize()), &free);
        assert!(pose(&ed, wall).0.abs_diff_eq(turned.0, 1e-3) && pose(&ed, wall).1.angle_between(turned.1) < 1e-3, "the drag carries on from the turned object");
        ed.drag_active = false;

        // The whole drag, turn included, is a single step.
        ed.commit_edits(false);
        ed.undo();
        let back = pose(&ed, wall);
        assert!(back.0.abs_diff_eq(start.0, 1e-3) && back.1.angle_between(start.1) < 1e-3, "undo puts the object back where it was before the drag, turn and move together");
        ed.redo();
        assert!(pose(&ed, wall).0.abs_diff_eq(turned.0, 1e-3) && pose(&ed, wall).1.angle_between(turned.1) < 1e-3, "and redo does the turned move again");

        // Without a drag the shortcut turns at once.
        ed.run_action(Action::Rotate90Y);
        assert!(ed.pending_turns.is_empty());
        assert!(pose(&ed, wall).1.angle_between(turned.1) > 1.5);
    }

    // ---- choosing a model's primary part ----

    #[test]
    fn the_primary_part_picker_only_takes_parts_inside_the_model() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = find(&ed, "Player");
        let (root, wall) = (ed.dm.borrow().find_child(model, "Body").unwrap(), find(&ed, "Wall"));
        let workspace = ed.dm.borrow().workspace;
        ed.dm.borrow_mut().set_prop(model, "PrimaryPart", Value::Ref(None)).unwrap();

        ed.begin_primary_pick(model);
        assert_eq!(ed.picking_primary, Some(model));
        assert!(ed.picking_any() && ed.picking_parent.is_none());
        assert!(ed.pick_ok(root), "a part of the model");
        assert!(!ed.pick_ok(wall) && !ed.pick_ok(model) && !ed.pick_ok(workspace), "not an unrelated part, the model itself or an ancestor");
        for bad in [wall, model, workspace] {
            assert!(!ed.pick_candidate(bad));
            assert_eq!(ed.picking_primary, Some(model), "a bad pick does not end the choosing");
            assert_eq!(ed.dm.borrow().reference(model, "PrimaryPart"), None);
        }
        assert!(ed.log.last().unwrap().text.contains("cannot be the primary part"));
        assert!(ed.pick_candidate(root));
        assert_eq!(ed.picking_primary, None);
        assert_eq!(ed.dm.borrow().reference(model, "PrimaryPart"), Some(root));
        assert!(ed.dirty);

        // Escape or Cancel gives up; choosing a parent and a primary part are one choice at a time.
        ed.begin_primary_pick(model);
        ed.cancel_pick();
        assert!(!ed.picking_any());
        ed.begin_primary_pick(model);
        ed.begin_parent_pick(vec![wall]);
        assert!(ed.picking_primary.is_none() && ed.picking_parent.is_some());
        ed.cancel_pick();
        // Only something with a primary part to choose starts it, and a model with no parts has nothing to offer.
        ed.begin_primary_pick(wall);
        assert!(!ed.picking_any(), "a part has no primary part");
        let empty = ed.dm.borrow_mut().create("Model").unwrap();
        ed.dm.borrow_mut().set_parent(empty, Some(workspace)).unwrap();
        ed.begin_primary_pick(empty);
        assert!(!ed.picking_any());
        assert!(ed.log.last().unwrap().text.contains("no part inside it"));
    }

    #[test]
    fn the_primary_part_follows_the_models_hierarchy_through_undo_and_the_panel_can_clear_it() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = find(&ed, "Player");
        let root = ed.dm.borrow().find_child(model, "Body").unwrap();
        assert_eq!(ed.dm.borrow().reference(model, "PrimaryPart"), Some(root));
        let workspace = ed.dm.borrow().workspace;

        // The part leaves the model: the reference is cleared with it, and a single undo puts both back.
        ed.dm.borrow_mut().set_parent(root, Some(workspace)).unwrap();
        ed.commit_edits(false);
        assert_eq!(ed.dm.borrow().get_prop(model, "PrimaryPart"), Some(Value::Ref(None)), "no stale reference");
        ed.undo();
        assert_eq!(ed.dm.borrow().parent(root), Some(model));
        assert_eq!(ed.dm.borrow().reference(model, "PrimaryPart"), Some(root), "the reference came back with the part");
        ed.redo();
        assert_eq!(ed.dm.borrow().reference(model, "PrimaryPart"), None);
        ed.undo();

        // Deleting the part clears it too, and undo restores it.
        ed.dm.borrow_mut().destroy(root);
        ed.commit_edits(false);
        assert_eq!(ed.dm.borrow().get_prop(model, "PrimaryPart"), Some(Value::Ref(None)));
        ed.undo();
        let back = ed.dm.borrow().find_child(model, "Body").unwrap();
        assert_eq!(ed.dm.borrow().reference(model, "PrimaryPart"), Some(back));

        // Cleared like any property (the panel's Clear button writes nil), and refused for what is not inside.
        ed.dm.borrow_mut().set_prop(model, "PrimaryPart", Value::Ref(None)).unwrap();
        let wall = find(&ed, "Wall");
        assert!(ed.dm.borrow_mut().set_prop(model, "PrimaryPart", Value::Ref(Some(wall))).is_err(), "the data layer refuses it whatever the way in");
    }

    // ---- scaling a model ----

    #[test]
    fn scaling_a_model_is_one_undo_step_and_survives_duplicating_saving_and_reopening() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = find(&ed, "Player");
        let root = ed.dm.borrow().find_child(model, "Body").unwrap();
        let (size, pos) = (ed.dm.borrow().size(root), ed.dm.borrow().vec3(root, "Position"));
        let pivot = ed.dm.borrow().get_pivot(model).unwrap().0;
        ed.select_only(Some(model));

        ed.dm.borrow_mut().set_prop(model, "Scale", Value::Num(2.0)).unwrap();
        ed.commit_edits(false);
        assert_eq!(ed.dm.borrow().size(root), size * 2.0);
        assert!(ed.dm.borrow().get_pivot(model).unwrap().0.abs_diff_eq(pivot, 1e-3), "the pivot stays");
        ed.undo();
        assert_eq!(ed.dm.borrow().size(root), size, "one undo takes the whole scaling back");
        assert_eq!(ed.dm.borrow().get_prop(model, "Scale"), Some(Value::Num(1.0)));
        assert!(ed.dm.borrow().vec3(root, "Position").abs_diff_eq(pos, 1e-3));
        ed.redo();
        assert_eq!(ed.dm.borrow().size(root), size * 2.0);
        assert_eq!(ed.dm.borrow().get_prop(model, "Scale"), Some(Value::Num(2.0)));

        // A copy is as big as the original (and scaling the copy leaves the original).
        ed.run_action(Action::Duplicate);
        let copy = ed.selected.unwrap();
        let copy_root = ed.dm.borrow().find_child(copy, "Body").unwrap();
        assert_eq!(ed.dm.borrow().get_prop(copy, "Scale"), Some(Value::Num(2.0)));
        assert_eq!(ed.dm.borrow().size(copy_root), size * 2.0);
        ed.dm.borrow_mut().set_prop(copy, "Scale", Value::Num(1.0)).unwrap();
        assert_eq!((ed.dm.borrow().size(copy_root), ed.dm.borrow().size(root)), (size, size * 2.0));
        ed.dm.borrow_mut().destroy(copy);

        // Saved and opened again: the same, and not scaled a second time.
        ed.dirty = true;
        ed.save_all();
        let later = editor(&s);
        let model2 = find(&later, "Player");
        let root2 = later.dm.borrow().find_child(model2, "Body").unwrap();
        assert_eq!(later.dm.borrow().size(root2), size * 2.0);
        assert_eq!(later.dm.borrow().get_prop(model2, "Scale"), Some(Value::Num(2.0)));
    }
}