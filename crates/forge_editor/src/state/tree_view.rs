//! What the Explorer lists, and how clicks in it change the selection.
//!
//! * [`visible_rows`] is the list of rows on screen, top to bottom: the services the Explorer exposes
//!   (in their engine order), what is inside the ones that are open, or - while a [`TreeFilter`] is
//!   active - the objects that match plus the objects that contain them.
//! * [`Editor::click_row`] is what a click on a row does: plain, `Ctrl` (add / remove one) and `Shift`
//!   (a range of the visible rows, from the anchor), and `Ctrl + Shift` (add a range).
//!
//! None of it changes the scene: a filter only decides what is drawn, and a collapsed node only hides
//! its children from the list.

use std::collections::HashSet;

use forge_engine::{
    core::ResolvedClass,
    dm::{DataModel, Id},
};

use super::Editor;

/// The Explorer's search box and its settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeFilter {
    pub text: String,
    pub match_case: bool,
    /// The text is looked for in the object's name ...
    pub in_name: bool,
    /// ... and / or in the name of its class.
    pub in_class: bool,
    /// Only objects of this class (or one that inherits from it): `BasePart` finds `Part` and `MeshPart`.
    pub class: Option<String>,
}

impl Default for TreeFilter {
    fn default() -> Self {
        TreeFilter { text: String::new(), match_case: false, in_name: true, in_class: false, class: None }
    }
}

impl TreeFilter {
    /// Whether anything is being filtered.
    pub fn is_active(&self) -> bool {
        !self.text.trim().is_empty() || self.class.is_some()
    }

    /// Whether a setting differs from the starting one (the text itself does not count).
    pub fn has_options(&self) -> bool {
        let d = TreeFilter::default();
        self.match_case != d.match_case || self.in_name != d.in_name || self.in_class != d.in_class || self.class.is_some()
    }

    /// Clears the text and every setting.
    pub fn reset(&mut self) {
        *self = TreeFilter::default();
    }

    /// Whether an object called `name` of class `class` is found.
    pub fn matches(&self, name: &str, class: &ResolvedClass) -> bool {
        if let Some(want) = &self.class {
            if !class.is_a(want) {
                return false;
            }
        }
        let text = self.text.trim();
        if text.is_empty() {
            return true;
        }
        // (With both switches off there would be nothing to find: the name is what is searched.)
        let (in_name, in_class) = (self.in_name || !self.in_class, self.in_class);
        (in_name && contains(name, text, self.match_case)) || (in_class && contains(class.name, text, self.match_case))
    }
}

/// `haystack` contains `needle`, optionally ignoring case. (No allocation for plain ASCII, which is
/// nearly every name: this runs for every object on every frame while a filter is active.)
fn contains(haystack: &str, needle: &str, match_case: bool) -> bool {
    if match_case {
        return haystack.contains(needle);
    }
    if haystack.is_ascii() && needle.is_ascii() {
        let n = needle.as_bytes();
        return n.is_empty() || haystack.as_bytes().windows(n.len()).any(|w| w.eq_ignore_ascii_case(n));
    }
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// One line of the Explorer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub id: Id,
    pub depth: usize,
    /// Only listed because something inside it matches the filter (it does not match itself).
    pub context: bool,
}

/// The services the Explorer lists, in their engine order.
fn exposed_services(dm: &DataModel) -> Vec<Id> {
    dm.get(dm.game).map(|g| g.children.iter().copied().filter(|c| dm.get(*c).is_some_and(|i| !i.class.desc.hidden_in_explorer)).collect()).unwrap_or_default()
}

/// The rows of the Explorer, top to bottom. Costs time in proportion to the rows listed (or, while
/// filtering, to the objects searched), and copies nothing per object.
pub fn visible_rows(dm: &DataModel, collapsed: &HashSet<u32>, filter: &TreeFilter) -> Vec<TreeRow> {
    let roots = exposed_services(dm);
    if !filter.is_active() {
        let mut rows = Vec::new();
        let mut stack: Vec<(Id, usize)> = roots.iter().rev().map(|c| (*c, 0)).collect();
        while let Some((id, depth)) = stack.pop() {
            let Some(inst) = dm.get(id) else { continue };
            rows.push(TreeRow { id, depth, context: false });
            if !inst.children.is_empty() && !collapsed.contains(&inst.uid) {
                stack.extend(inst.children.iter().rev().map(|c| (*c, depth + 1)));
            }
        }
        return rows;
    }

    // Filtering searches the whole tree, whatever is collapsed: every object that matches is listed
    // under the chain of objects that contain it. (Collapsing is left alone, so clearing the filter
    // brings back the tree as it was.)
    struct Node {
        id: Id,
        depth: usize,
        parent: Option<usize>,
        found: bool,
        keep: bool,
    }
    let mut nodes: Vec<Node> = Vec::new();
    let mut stack: Vec<(Id, usize, Option<usize>)> = roots.iter().rev().map(|c| (*c, 0, None)).collect();
    while let Some((id, depth, parent)) = stack.pop() {
        let Some(inst) = dm.get(id) else { continue };
        let found = filter.matches(&inst.name, inst.class);
        let at = nodes.len();
        nodes.push(Node { id, depth, parent, found, keep: found });
        stack.extend(inst.children.iter().rev().map(|c| (*c, depth + 1, Some(at))));
    }
    // Parents come before their children, so one backwards pass hands "something inside matches" upwards.
    for i in (0..nodes.len()).rev() {
        if nodes[i].keep {
            if let Some(p) = nodes[i].parent {
                nodes[p].keep = true;
            }
        }
    }
    nodes.into_iter().filter(|n| n.keep).map(|n| TreeRow { id: n.id, depth: n.depth, context: !n.found }).collect()
}

/// The names of the classes a "type" filter can offer: the class of every object in the scene and the
/// classes it inherits from (so `BasePart` is offered when there are `Part`s), without the root classes.
pub fn classes_in_scene(dm: &DataModel) -> Vec<&'static str> {
    let mut seen: HashSet<&'static str> = HashSet::new();
    for id in dm.all_ids() {
        if let Some(inst) = dm.get(id) {
            seen.extend(inst.class.chain.iter().filter(|c| !c.root && c.name != "Object").map(|c| c.name));
        }
    }
    let mut names: Vec<&'static str> = seen.into_iter().collect();
    names.sort_unstable();
    names
}

impl Editor {
    /// The rows the Explorer lists right now.
    pub fn explorer_rows(&self) -> Vec<TreeRow> {
        visible_rows(&self.dm.borrow(), &self.collapsed, &self.tree_filter)
    }

    /// A click on a row of the Explorer. `rows` is what the Explorer lists (see [`Editor::explorer_rows`]).
    /// * plain: select only this object;
    /// * `Ctrl`: add it to the selection, or take it out if it is selected;
    /// * `Shift`: select every row from the anchor (the last row clicked without Shift) to this one;
    /// * `Ctrl + Shift`: the same, keeping what was already selected.
    pub fn click_row(&mut self, rows: &[TreeRow], id: Id, ctrl: bool, shift: bool) {
        match (ctrl, shift) {
            (false, false) => {
                self.select_only(Some(id));
                self.anchor = Some(id);
            }
            (true, false) => {
                self.select_toggle(id);
                self.anchor = Some(id);
            }
            (_, true) => self.select_range(rows, id, ctrl),
        }
    }

    /// Selects the rows between the anchor and `to`, inclusive. `to` becomes the active object and the
    /// anchor stays where it was, so the range can be stretched or shrunk with further Shift-clicks. When
    /// the anchor is not on screen any more (collapsed away, filtered out, deleted) the active object
    /// stands in for it, and failing that only `to` is selected.
    pub fn select_range(&mut self, rows: &[TreeRow], to: Id, keep_selection: bool) {
        self.normalize_selection();
        let position = |id: Id| rows.iter().position(|r| r.id == id);
        let from = self.anchor.and_then(position).or_else(|| self.selected.and_then(position));
        let (Some(to_at), Some(from_at)) = (position(to), from) else {
            self.select_only(Some(to));
            self.anchor = Some(to);
            return;
        };
        if self.anchor.and_then(position).is_none() {
            self.anchor = self.selected;
        }
        let (lo, hi) = (from_at.min(to_at), from_at.max(to_at));
        let mut chosen: Vec<Id> = Vec::with_capacity(hi - lo + 1);
        if keep_selection {
            chosen.extend(self.selection.iter().copied().filter(|s| *s != to));
        }
        let have: HashSet<Id> = chosen.iter().copied().collect();
        chosen.extend(rows[lo..=hi].iter().map(|r| r.id).filter(|r| *r != to && !have.contains(r)));
        chosen.push(to);
        self.selection = chosen;
        self.selected = Some(to);
        self.last_inspected = Some(to);
    }

    /// A right-click on a row: a row outside the selection becomes the selection; a row inside it
    /// becomes the active object and the rest of the selection stays (so the menu acts on all of them).
    pub fn context_row(&mut self, id: Id) {
        self.normalize_selection();
        if self.is_selected(id) {
            self.selected = Some(id);
        } else {
            self.select_only(Some(id));
            self.anchor = Some(id);
        }
    }

    /// How many of the selected objects the Explorer does not list at the moment (collapsed away or
    /// filtered out). They stay selected, and whatever acts on the selection acts on them too.
    pub fn selected_but_unlisted(&self, rows: &[TreeRow]) -> usize {
        if self.selection.is_empty() {
            return 0;
        }
        let listed: HashSet<Id> = rows.iter().map(|r| r.id).collect();
        self.selection_ids().iter().filter(|s| !listed.contains(s)).count()
    }

    /// Opens every container above `id`, so its row is listed.
    pub fn expand_to(&mut self, id: Id) {
        let dm = self.dm.borrow();
        let mut cur = dm.parent(id);
        while let Some(p) = cur {
            if let Some(i) = dm.get(p) {
                self.collapsed.remove(&i.uid);
            }
            cur = dm.parent(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use forge_engine::{core::Value, project::Project};

    use super::*;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_treeview_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Tree Test").expect("create project");
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

    fn names(ed: &Editor, rows: &[TreeRow]) -> Vec<String> {
        rows.iter().map(|r| ed.dm.borrow().name(r.id)).collect()
    }

    /// A chain of `depth` nested models under the workspace, with a part at the bottom. Returns the part.
    fn deep_chain(ed: &Editor, depth: usize) -> Id {
        let mut dm = ed.dm.borrow_mut();
        let mut parent = dm.workspace;
        for i in 0..depth {
            let m = dm.create("Model").unwrap();
            dm.set_name(m, &format!("Level {i}"));
            dm.set_parent(m, Some(parent)).unwrap();
            parent = m;
        }
        let part = dm.create("Part").unwrap();
        dm.set_name(part, "Deep Part");
        dm.set_parent(part, Some(parent)).unwrap();
        part
    }

    #[test]
    fn the_explorer_lists_the_chosen_services_in_order_and_hides_the_scripting_ones() {
        let s = Scratch::new();
        let ed = editor(&s);
        let rows = ed.explorer_rows();
        let top: Vec<String> = rows.iter().filter(|r| r.depth == 0).map(|r| ed.dm.borrow().name(r.id)).collect();
        assert_eq!(
            top,
            ["Workspace", "Players", "Lighting", "ReplicatedFirst", "ReplicatedStorage", "ServerScriptService", "ServerStorage", "StarterGui", "StarterPlayer", "SoundService"]
        );
        let dm = ed.dm.borrow();
        for hidden in ["RunService", "UserInputService", "CollectionService", "Debris", "TweenService"] {
            assert!(dm.service(hidden).is_some(), "{hidden} still exists for scripts");
            assert!(!top.iter().any(|t| t == hidden), "{hidden} is not listed");
        }
        assert!(rows.iter().all(|r| !r.context));
    }

    #[test]
    fn every_listed_service_is_a_real_service_you_can_put_things_in() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        for name in ["ReplicatedFirst", "StarterGui", "SoundService"] {
            let id = ed.dm.borrow().service(name).unwrap_or_else(|| panic!("{name} is missing"));
            assert!(ed.dm.borrow().get(id).unwrap().class.desc.container, "{name} takes new objects");
            ed.select_only(Some(id));
            ed.insert("Folder");
            let folder = ed.selected.expect("the new folder is selected");
            assert_eq!(ed.dm.borrow().parent(folder), Some(id), "{name} received the folder");
        }
        // They are saved with the scene and come back.
        let text = serde_json::to_string(&ed.dm.borrow().to_place("Main")).unwrap();
        assert!(text.contains("StarterGui") && text.contains("SoundService") && text.contains("ReplicatedFirst"));
    }

    #[test]
    fn collapsing_hides_children_and_the_filter_leaves_the_collapsed_state_alone() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let part = deep_chain(&ed, 6);
        let uid = ed.dm.borrow().get(ed.dm.borrow().workspace).unwrap().uid;
        let all = ed.explorer_rows();
        assert!(all.iter().any(|r| r.id == part));
        ed.collapsed.insert(uid);
        let collapsed = ed.explorer_rows();
        assert!(collapsed.len() < all.len() && !collapsed.iter().any(|r| r.id == part), "the workspace is closed");

        // A search finds it anyway, with the chain of containers above it as context, in tree order.
        ed.tree_filter.text = "deep".into();
        let found = ed.explorer_rows();
        let listed = names(&ed, &found);
        assert_eq!(listed, ["Workspace", "Level 0", "Level 1", "Level 2", "Level 3", "Level 4", "Level 5", "Deep Part"]);
        assert_eq!(found.iter().map(|r| r.depth).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
        assert_eq!(found.iter().filter(|r| !r.context).count(), 1, "only the part itself matches; the rest is context");
        assert!(ed.collapsed.contains(&uid), "searching did not expand anything for good");

        ed.tree_filter.reset();
        assert_eq!(ed.explorer_rows(), collapsed, "clearing the search brings the tree back as it was");
    }

    #[test]
    fn the_filter_matches_names_classes_and_types_and_never_touches_the_scene() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let before = ed.dm.borrow().all_ids().len();
        let matching = |ed: &Editor| -> Vec<String> { ed.explorer_rows().iter().filter(|r| !r.context).map(|r| ed.dm.borrow().name(r.id)).collect() };

        ed.tree_filter.text = "WALL".into();
        assert_eq!(matching(&ed), ["Wall"], "case is ignored by default");
        ed.tree_filter.match_case = true;
        assert!(matching(&ed).is_empty(), "and matters when asked");
        ed.tree_filter.text = "Wall".into();
        assert_eq!(matching(&ed), ["Wall"]);
        ed.tree_filter.reset();

        // The class name is searched only when asked for.
        ed.tree_filter.text = "meshpart".into();
        assert!(matching(&ed).is_empty());
        ed.tree_filter.in_class = true;
        ed.tree_filter.in_name = false;
        {
            let mesh = ed.dm.borrow_mut().create("MeshPart").unwrap();
            let ws = ed.dm.borrow().workspace;
            ed.dm.borrow_mut().set_name(mesh, "Statue");
            ed.dm.borrow_mut().set_parent(mesh, Some(ws)).unwrap();
        }
        assert_eq!(matching(&ed), ["Statue"]);

        // A type finds every object that is one, subclasses included.
        ed.tree_filter.reset();
        ed.tree_filter.class = Some("BasePart".into());
        let parts = matching(&ed);
        assert!(parts.contains(&"Wall".to_owned()) && parts.contains(&"Statue".to_owned()), "{parts:?}");
        assert!(!parts.contains(&"Workspace".to_owned()));
        ed.tree_filter.text = "step".into();
        assert!(matching(&ed).iter().all(|n| n.to_lowercase().contains("step")), "type and text must both hold");
        assert!(classes_in_scene(&ed.dm.borrow()).contains(&"BasePart"), "the type list offers base classes");

        ed.tree_filter.reset();
        assert_eq!(ed.dm.borrow().all_ids().len(), before + 1, "filtering added and removed nothing (only the Statue made here)");
    }

    #[test]
    fn plain_and_ctrl_clicks_select_one_or_toggle() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let rows = ed.explorer_rows();
        let (a, b, c) = (find(&ed, "Wall"), find(&ed, "Step 1"), find(&ed, "Step 2"));
        ed.click_row(&rows, a, false, false);
        assert_eq!(ed.selection, vec![a]);
        ed.click_row(&rows, b, true, false);
        ed.click_row(&rows, c, true, false);
        assert_eq!(ed.selection, vec![a, b, c]);
        ed.click_row(&rows, b, true, false);
        assert_eq!(ed.selection, vec![a, c], "ctrl on a selected row takes it out");
        ed.click_row(&rows, b, false, false);
        assert_eq!(ed.selection, vec![b], "a plain click replaces everything");
    }

    #[test]
    fn shift_selects_the_rows_between_the_anchor_and_the_click_in_both_directions() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let rows = ed.explorer_rows();
        let id_at = |i: usize| rows[i].id;
        let wall_at = rows.iter().position(|r| r.id == find(&ed, "Wall")).unwrap();

        ed.click_row(&rows, id_at(wall_at), false, false);
        ed.click_row(&rows, id_at(wall_at + 3), false, true);
        let want: Vec<Id> = (wall_at..=wall_at + 3).map(id_at).collect();
        assert_eq!(ed.selection.iter().copied().collect::<HashSet<_>>(), want.iter().copied().collect::<HashSet<_>>());
        assert_eq!(ed.selected, Some(id_at(wall_at + 3)), "the clicked row is the active one");
        assert_eq!(ed.anchor, Some(id_at(wall_at)), "the anchor stays");

        // Shift again from the same anchor shrinks / turns the range round.
        ed.click_row(&rows, id_at(wall_at + 1), false, true);
        assert_eq!(ed.selection.len(), 2);
        ed.click_row(&rows, id_at(wall_at - 2), false, true);
        assert_eq!(ed.selection.len(), 3, "upwards from the anchor");
        assert!(ed.selection.contains(&id_at(wall_at - 2)) && ed.selection.contains(&id_at(wall_at)));

        // Ctrl + Shift keeps what was selected elsewhere.
        ed.click_row(&rows, id_at(wall_at + 6), true, false);
        ed.click_row(&rows, id_at(wall_at + 8), true, true);
        assert!(ed.selection.contains(&id_at(wall_at)) && ed.selection.contains(&id_at(wall_at + 7)) && ed.selection.contains(&id_at(wall_at + 8)));
        assert!(!ed.selection.contains(&id_at(wall_at + 4)), "the rows between the two groups stay unselected");
    }

    #[test]
    fn a_range_only_covers_listed_rows_so_collapsed_children_are_not_swept_in() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = find(&ed, "Player");
        let inside = ed.dm.borrow().find_child(model, "Body").unwrap();
        let (wall, step) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        let model_uid = ed.dm.borrow().get(model).unwrap().uid;
        ed.collapsed.insert(model_uid);
        let rows = ed.explorer_rows();
        assert!(!rows.iter().any(|r| r.id == inside), "the model is closed");

        let (first, last) = {
            let at = |id: Id| rows.iter().position(|r| r.id == id).unwrap();
            if at(wall) < at(model) { (wall, model) } else { (model, wall) }
        };
        ed.click_row(&rows, first, false, false);
        ed.click_row(&rows, last, false, true);
        assert!(ed.selection.contains(&model) && !ed.selection.contains(&inside), "a closed model's contents are not selected by a range over it");
        // Nor does clicking rows expand or collapse anything.
        assert!(ed.collapsed.contains(&model_uid));
        let _ = step;

        // Opening it lists the children, and a range over them selects them.
        ed.collapsed.remove(&model_uid);
        let rows = ed.explorer_rows();
        ed.click_row(&rows, model, false, false);
        ed.click_row(&rows, inside, false, true);
        assert!(ed.selection.contains(&model) && ed.selection.contains(&inside));
    }

    #[test]
    fn a_range_works_when_the_anchor_is_gone_hidden_or_was_never_set() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b, c) = (find(&ed, "Wall"), find(&ed, "Step 1"), find(&ed, "Step 2"));
        let rows = ed.explorer_rows();

        // No anchor and no selection: just the clicked row.
        ed.select_only(None);
        ed.anchor = None;
        ed.click_row(&rows, b, false, true);
        assert_eq!(ed.selection, vec![b]);

        // The anchor was deleted: the active object stands in.
        ed.click_row(&rows, a, false, false);
        ed.select_toggle(c);
        ed.dm.borrow_mut().destroy(a);
        let rows = ed.explorer_rows();
        ed.click_row(&rows, b, false, true);
        assert!(ed.selection.contains(&b) && ed.selection.contains(&c) && !ed.selection.iter().any(|s| *s == a));
        assert_eq!(ed.selected, Some(b));

        // The anchor is filtered out: also falls back to the active object.
        ed.click_row(&rows, c, false, false);
        ed.tree_filter.text = "step 1".into();
        let rows = ed.explorer_rows();
        ed.click_row(&rows, b, false, true);
        assert!(ed.selection.contains(&b) && ed.selected == Some(b));
    }

    #[test]
    fn right_clicking_keeps_a_multi_selection_it_is_part_of_and_selects_anything_else() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b, c) = (find(&ed, "Wall"), find(&ed, "Step 1"), find(&ed, "Step 2"));
        ed.select_only(Some(a));
        ed.select_toggle(b);
        ed.context_row(a);
        assert_eq!((ed.selection.len(), ed.selected), (2, Some(a)), "inside the selection: nothing is lost");
        ed.context_row(c);
        assert_eq!((ed.selection.clone(), ed.selected), (vec![c], Some(c)), "outside it: that row is selected");
    }

    #[test]
    fn selection_survives_renames_moves_and_filters_and_unlisted_ones_are_counted() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        let model = find(&ed, "Player");
        ed.select_only(Some(a));
        ed.select_toggle(b);

        ed.dm.borrow_mut().set_prop(a, "Name", Value::Str("Fence".into())).unwrap();
        ed.reparent(a, model);
        ed.normalize_selection();
        assert_eq!(ed.selection, vec![a, b], "a rename and a move keep the same objects selected");

        // A filter that hides one of them does not deselect it, but says so.
        ed.tree_filter.text = "fence".into();
        let rows = ed.explorer_rows();
        assert_eq!(ed.selected_but_unlisted(&rows), 1);
        assert_eq!(ed.selection.len(), 2);

        // Deleting one drops only that one.
        ed.dm.borrow_mut().destroy(b);
        ed.normalize_selection();
        assert_eq!(ed.selection, vec![a]);
        assert_eq!(ed.selected_but_unlisted(&ed.explorer_rows()), 0);
    }

    #[test]
    fn dropping_objects_into_a_closed_container_opens_it() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, model) = (find(&ed, "Wall"), find(&ed, "Player"));
        let uid = ed.dm.borrow().get(model).unwrap().uid;
        ed.collapsed.insert(uid);
        ed.select_only(Some(a));
        ed.reparent(a, model);
        assert!(!ed.collapsed.contains(&uid));
        assert_eq!(ed.dm.borrow().parent(a), Some(model));
        assert_eq!(ed.selection, vec![a], "and it is still selected");
    }

    #[test]
    fn deep_hierarchies_and_big_scenes_stay_fast_and_correct() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let part = deep_chain(&ed, 400);
        let rows = ed.explorer_rows();
        assert_eq!(rows.iter().find(|r| r.id == part).map(|r| r.depth), Some(401), "400 levels deep");

        // 20,000 parts spread over folders.
        {
            let mut dm = ed.dm.borrow_mut();
            let ws = dm.workspace;
            for f in 0..40 {
                let folder = dm.create("Folder").unwrap();
                dm.set_name(folder, &format!("Group {f}"));
                dm.set_parent(folder, Some(ws)).unwrap();
                for p in 0..500 {
                    let part = dm.create("Part").unwrap();
                    dm.set_name(part, &format!("Brick {f}-{p}"));
                    dm.set_parent(part, Some(folder)).unwrap();
                }
            }
        }
        let started = std::time::Instant::now();
        let all = ed.explorer_rows();
        assert!(all.len() > 20_000);
        ed.tree_filter.text = "brick 39-499".into();
        let found = ed.explorer_rows();
        let elapsed = started.elapsed();
        assert_eq!(found.iter().filter(|r| !r.context).count(), 1);
        assert_eq!(found.len(), 3, "the part, its folder and the workspace");
        assert!(elapsed.as_millis() < 500, "listing and searching 20,000 objects took {elapsed:?}");

        // Select everything listed in one range and act on it.
        ed.tree_filter.reset();
        let rows = ed.explorer_rows();
        ed.click_row(&rows, rows[0].id, false, false);
        ed.click_row(&rows, rows[rows.len() - 1].id, false, true);
        assert_eq!(ed.selection.len(), rows.len());
        let started = std::time::Instant::now();
        let top = ed.top_level_selection();
        assert!(started.elapsed().as_millis() < 500, "working out the top level of {} selected objects took {:?}", ed.selection.len(), started.elapsed());
        assert!(top.len() < ed.selection.len());
    }
}
