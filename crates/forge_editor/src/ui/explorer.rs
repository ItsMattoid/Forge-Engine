//! The Explorer: the instance tree with search, multi-selection, drag-to-reparent and a context menu.
//!
//! What it lists (the services, the filter) and what a click does to the selection live in
//! `state::tree_view`; this file draws the rows and passes the clicks on.

use std::collections::HashSet;

use bevy_egui::egui;
use forge_engine::dm::Id;

use super::menus::insert_menu;
use crate::{
    state::{classes_in_scene, AssetDrag, Editor, TreeRow},
    theme::{secondary_color, SecondaryText},
};
use super::assets::{drop_tint, set_drag_hint};

struct Row {
    id: Id,
    depth: usize,
    has_kids: bool,
    name: String,
    service: bool,
    context: bool,
}

/// The row of an instance (built only for the rows that are on screen).
fn row_of(ed: &Editor, row: &TreeRow) -> Option<Row> {
    let dm = ed.dm.borrow();
    let inst = dm.get(row.id)?;
    Some(Row {
        id: row.id,
        depth: row.depth,
        has_kids: !inst.children.is_empty(),
        name: inst.name.clone(),
        service: inst.class.desc.service_order.is_some(),
        context: row.context,
    })
}

/// The instance ids (by uid) of everything that has a selected object somewhere inside it.
fn holders_of_selection(ed: &Editor) -> HashSet<u32> {
    let dm = ed.dm.borrow();
    let mut out = HashSet::new();
    for id in &ed.selection {
        let mut cur = dm.parent(*id);
        while let Some(p) = cur {
            // (Stops at the first one already known: everything above it is known too.)
            if let Some(i) = dm.get(p) {
                if !out.insert(i.uid) {
                    break;
                }
            }
            cur = dm.parent(p);
        }
    }
    out
}

/// The search box, the filter settings and the Insert menu.
fn search_bar(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.horizontal(|ui| {
        ui.menu_button("+ Insert", |ui| insert_menu(ui, ed));
        let width = (ui.available_width() - 84.0).max(60.0);
        let hint = if ed.tree_filter.in_class && !ed.tree_filter.in_name { "Search by class..." } else { "Search..." };
        ui.add(egui::TextEdit::singleline(&mut ed.tree_filter.text).hint_text(hint).desired_width(width));
        if ui.add_enabled(ed.tree_filter.is_active(), egui::Button::new("x").small()).on_hover_text("Clear the search").clicked() {
            ed.tree_filter.reset();
        }
        let label = if ed.tree_filter.has_options() { "Filter *" } else { "Filter" };
        ui.menu_button(label, |ui| {
            let filter = &mut ed.tree_filter;
            ui.checkbox(&mut filter.match_case, "Match case");
            ui.separator();
            ui.label("Search in");
            ui.checkbox(&mut filter.in_name, "Name");
            ui.checkbox(&mut filter.in_class, "Class");
            // (Something has to be searched: the last box cannot be switched off.)
            if !filter.in_name && !filter.in_class {
                filter.in_name = true;
            }
            ui.separator();
            ui.label("Type");
            let shown = filter.class.clone().unwrap_or_else(|| "Any type".to_owned());
            egui::ComboBox::from_id_salt("explorer_type_filter").selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut filter.class, None, "Any type");
                for name in classes_in_scene(&ed.dm.borrow()) {
                    ui.selectable_value(&mut filter.class, Some(name.to_owned()), name);
                }
            });
            ui.separator();
            if ui.add_enabled(filter.is_active() || filter.has_options(), egui::Button::new("Reset filter")).clicked() {
                filter.reset();
            }
        });
    });
}

pub(super) fn explorer(ui: &mut egui::Ui, ed: &mut Editor) {
    ed.normalize_selection();
    // A rename or a range anchor that points at something deleted is dropped, not left to dangle.
    if ed.renaming.as_ref().is_some_and(|(id, _)| !ed.dm.borrow().exists(*id)) {
        ed.renaming = None;
    }
    if ed.anchor.is_some_and(|a| !ed.dm.borrow().exists(a)) {
        ed.anchor = None;
    }

    ui.heading("Explorer");
    search_bar(ui, ed);
    ui.separator();

    // Selecting in the viewport expands the tree down to the selection.
    let reveal = std::mem::take(&mut ed.reveal);
    if reveal {
        if let Some(sel) = ed.selected {
            ed.expand_to(sel);
        }
    }

    let rows = ed.explorer_rows();
    let filtering = ed.tree_filter.is_active();

    // What the list is showing: the matches, and how much of the selection is out of sight.
    let mut notes: Vec<String> = Vec::new();
    if filtering {
        let found = rows.iter().filter(|r| !r.context).count();
        notes.push(if found == 0 { "Nothing matches.".to_owned() } else { format!("{found} match{}", if found == 1 { "" } else { "es" }) });
    }
    if filtering || ed.selection.len() > 1 {
        let selected = ed.selection_ids().len();
        let unlisted = ed.selected_but_unlisted(&rows);
        if selected > 1 || unlisted > 0 {
            notes.push(format!("{selected} selected"));
        }
        if unlisted > 0 {
            notes.push(format!("{unlisted} not shown"));
        }
    }
    if !notes.is_empty() {
        ui.secondary(notes.join("  |  "));
    }

    // Closed rows that have something selected inside get a mark (their name in the accent colour).
    let holders = if filtering || ed.collapsed.is_empty() { HashSet::new() } else { holders_of_selection(ed) };

    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
    if reveal {
        if let Some(i) = ed.selected.and_then(|s| rows.iter().position(|r| r.id == s)) {
            area = area.vertical_scroll_offset((i as f32 * row_h - 120.0).max(0.0));
        }
    }

    let mut reparent: Option<(Id, Id)> = None;
    let mut asset_drop: Option<(Vec<String>, Id)> = None;
    let mut rename_done: Option<bool> = None;
    area.show_rows(ui, row_h, rows.len(), |ui, range| {
        for tree_row in &rows[range] {
            let Some(r) = row_of(ed, tree_row) else { continue };
            ui.horizontal(|ui| {
                ui.add_space(r.depth as f32 * 14.0);
                if r.has_kids && !filtering {
                    let open = !ed.collapsed.contains(&r.id.uid);
                    if ui.small_button(if open { "v" } else { ">" }).clicked() && !ed.collapsed.remove(&r.id.uid) {
                        ed.collapsed.insert(r.id.uid);
                    }
                } else {
                    ui.add_space(20.0);
                }
                // Being renamed: a text box instead of the name (Enter or clicking away keeps it, Esc cancels).
                if let Some((_, typed)) = ed.renaming.as_mut().filter(|(id, _)| *id == r.id) {
                    let edit = ui.add(egui::TextEdit::singleline(typed).desired_width(160.0));
                    if !edit.has_focus() && !edit.lost_focus() {
                        edit.request_focus();
                    }
                    if edit.lost_focus() {
                        rename_done = Some(!ui.input(|i| i.key_pressed(egui::Key::Escape)));
                    }
                    return;
                }
                // The active object (the one the gizmo and the main property values come from) is
                // brighter than the others in a multi-selection.
                let active = ed.selected == Some(r.id);
                let selected = ed.is_selected(r.id);
                let mut text = egui::RichText::new(&r.name);
                if selected {
                    let bg = ui.visuals().selection.bg_fill;
                    text = text.background_color(if active { bg } else { bg.gamma_multiply(0.55) }).color(ui.visuals().selection.stroke.color);
                    if active {
                        text = text.strong();
                    }
                } else if r.context {
                    // Listed only because something inside it matches the search.
                    text = text.color(secondary_color(ui));
                } else if holders.contains(&r.id.uid) && ed.collapsed.contains(&r.id.uid) {
                    text = text.color(ui.visuals().selection.stroke.color);
                } else if r.service {
                    text = text.strong();
                }
                let resp = ui.add(egui::Label::new(text).selectable(false).sense(egui::Sense::click_and_drag()));
                // Choosing a parent (Properties > Parent): this click picks the row instead of selecting it.
                if ed.picking_any() {
                    if resp.hovered() {
                        drop_tint(ui, resp.rect, ed.pick_ok(r.id));
                    }
                    if resp.clicked() {
                        ed.pick_candidate(r.id);
                    }
                } else if resp.clicked() {
                    // Ctrl (or Alt) + click adds the object to the selection or takes it out, Shift + click selects
                    // the rows from the last one clicked, a plain click selects only it.
                    let m = ui.input(|i| i.modifiers);
                    ed.click_row(&rows, r.id, m.ctrl || m.command || m.alt, m.shift);
                }
                if resp.secondary_clicked() {
                    ed.context_row(r.id);
                }
                if !r.service {
                    resp.dnd_set_drag_payload(r.id);
                }
                if let Some(dragged) = resp.dnd_release_payload::<Id>() {
                    reparent = Some((*dragged, r.id));
                }
                // Assets from the Assets tab: green when dropping them here does something, red when not.
                if let Some(drag) = resp.dnd_hover_payload::<AssetDrag>() {
                    match ed.check_drop(&drag.paths, Some(r.id)) {
                        Ok(what) => {
                            drop_tint(ui, resp.rect, true);
                            set_drag_hint(ui.ctx(), true, what);
                        }
                        Err(why) => {
                            drop_tint(ui, resp.rect, false);
                            set_drag_hint(ui.ctx(), false, why);
                        }
                    }
                }
                if let Some(drag) = resp.dnd_release_payload::<AssetDrag>() {
                    asset_drop = Some((drag.paths.clone(), r.id));
                }
                resp.context_menu(|ui| {
                    ui.menu_button("Insert", |ui| insert_menu(ui, ed));
                    if !r.service {
                        if ui.button("Save as Prefab").on_hover_text("Save the selected objects as a file in Scenes/Prefabs that can be placed in any scene").clicked() {
                            ed.save_selection_as_prefab();
                            ui.close();
                        }
                        if ui.button("Duplicate").clicked() {
                            ed.duplicate(r.id);
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            ed.delete(r.id);
                            ui.close();
                        }
                    }
                    if ui.button("Focus").clicked() {
                        ed.focus_request = true;
                        ui.close();
                    }
                });
            });
        }
    });
    if let Some(accept) = rename_done {
        ed.finish_rename(accept);
    }
    if let Some((id, parent)) = reparent {
        ed.reparent(id, parent);
    }
    if let Some((paths, target)) = asset_drop {
        ed.drop_assets(&paths, Some(target), None);
    }
}

#[cfg(test)]
mod tests {
    use forge_engine::compat::RunFrame as _;
    use forge_engine::project::Project;

    use super::*;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_explorer_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Explorer Test").expect("create project");
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

    fn draw(ctx: &egui::Context, ed: &mut Editor) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 900.0))), ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| {
            egui::CentralPanel::default().show(&mut forge_engine::compat::root_ui(ctx), |ui| explorer(ui, ed));
        });
    }

    fn find(ed: &Editor, name: &str) -> Id {
        let dm = ed.dm.borrow();
        dm.find_first_child(dm.workspace, name, true).unwrap_or_else(|| panic!("no '{name}'"))
    }

    #[test]
    fn rows_show_the_name_and_no_class_label_and_the_names_stay_aligned() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let ctx = egui::Context::default();
        let raw = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 900.0))), ..Default::default() };
        let mut texts: Vec<(egui::Pos2, String)> = Vec::new();
        let (wall, step1) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        ed.select_only(Some(wall));
        ed.reveal = true;
        for _ in 0..3 {
            let out = ctx.run_frame(raw(), |ctx| {
                egui::CentralPanel::default().show(&mut forge_engine::compat::root_ui(ctx), |ui| explorer(ui, &mut ed));
            });
            texts = out.shapes.iter().filter_map(|s| if let egui::epaint::Shape::Text(t) = &s.shape { Some((t.pos, t.galley.text().to_owned())) } else { None }).collect();
        }
        let names: Vec<&str> = texts.iter().map(|(_, t)| t.as_str()).collect();
        assert!(names.contains(&"Wall") && names.contains(&"Step 1"), "the names are listed: {names:?}");
        assert!(!names.contains(&"Part"), "no class name next to the objects: {names:?}");
        let x = |name: &str| texts.iter().find(|(_, t)| t == name).map(|(p, _)| p.x).unwrap();
        assert!((x("Wall") - x("Step 1")).abs() < 0.5, "siblings line up");
        assert!(ed.is_selected(wall) && !ed.is_selected(step1));
    }

    #[test]
    fn the_explorer_draws_plain_filtered_and_with_a_multi_selection() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let ctx = egui::Context::default();
        draw(&ctx, &mut ed);
        draw(&ctx, &mut ed);

        let (a, b) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        ed.select_only(Some(a));
        ed.select_toggle(b);
        ed.reveal = true;
        draw(&ctx, &mut ed);

        ed.tree_filter.text = "step".into();
        draw(&ctx, &mut ed);
        draw(&ctx, &mut ed);
        assert_eq!(ed.selection.len(), 2, "drawing a filtered list does not change the selection");

        ed.tree_filter.in_class = true;
        ed.tree_filter.class = Some("BasePart".into());
        draw(&ctx, &mut ed);
        ed.tree_filter.text = "no such thing".into();
        draw(&ctx, &mut ed);
        assert_eq!(ed.selection.len(), 2);
    }

    #[test]
    fn a_pending_rename_or_anchor_for_a_deleted_object_is_dropped() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let ctx = egui::Context::default();
        let a = find(&ed, "Wall");
        ed.select_only(Some(a));
        ed.anchor = Some(a);
        ed.renaming = Some((a, "Fence".into()));
        ed.dm.borrow_mut().destroy(a);
        draw(&ctx, &mut ed);
        assert!(ed.renaming.is_none() && ed.anchor.is_none() && ed.selection.is_empty());
    }

    #[test]
    fn closed_containers_with_something_selected_inside_are_marked() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = find(&ed, "Player");
        let inside = ed.dm.borrow().find_child(model, "Body").unwrap();
        ed.select_only(Some(inside));
        let uid = ed.dm.borrow().get(model).unwrap().uid;
        ed.collapsed.insert(uid);
        let workspace_uid = ed.dm.borrow().get(ed.dm.borrow().workspace).unwrap().uid;
        let holders = holders_of_selection(&ed);
        assert!(holders.contains(&uid) && holders.contains(&workspace_uid));
        assert!(!holders.contains(&inside.uid));
    }
}
