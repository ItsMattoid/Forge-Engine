//! The Properties panel, generated entirely from class metadata.
//!
//! * One object selected: its properties, editable.
//! * Nothing selected: the last object that was selected stays on show (clearly marked as not
//!   selected) instead of the panel going blank.
//! * Several objects selected: the properties they all have (same name and type) are shown and
//!   editing one writes it to every selected object that allows it. Where the objects disagree the
//!   value is marked "mixed"; properties only some of them have are listed as not shared.

use bevy::math::Vec3;
use bevy_egui::egui::{self, Color32, RichText};
use crate::theme::SecondaryText;
use forge_engine::{
    core::{enum_names, PropMeta, PropType, ResolvedClass, ResolvedProp, Value, Widget},
    dm::{DataModel, Id},
};

use forge_engine::project::AssetKind;

use super::asset_field::{asset_field, FieldAction};
use crate::state::{Editor, PickTarget};

/// How many objects the reference picker lists at once.
const MAX_REF_OPTIONS: usize = 300;

const MIXED: Color32 = Color32::from_rgb(235, 175, 70);

/// What a property editor needs besides the value itself.
struct EditEnv<'a> {
    scripts: &'a [String],
    open_script: &'a mut Option<String>,
    /// A file field was clicked: the property and the kind of file it takes.
    pick: &'a mut Option<(&'static str, AssetKind)>,
    /// A material field's Edit button was clicked.
    open_material: &'a mut Option<String>,
    /// What the Parent control asked for this frame.
    parent_pick: &'a mut ParentPick,
}

/// The Parent property is not a drop-down: a button starts choosing, and the next click on an object in the
/// Explorer or the viewport is the new parent.
#[derive(PartialEq, Eq)]
enum ParentPick {
    Nothing,
    Begin,
    /// Choose the primary part of the active model (the same way, but only parts inside it are taken).
    BeginPrimary,
    Cancel,
}

/// Whether every object has a property called `meta.name` of the same type.
fn shared_by_all(dm: &DataModel, targets: &[Id], meta: &PropMeta) -> bool {
    targets.iter().all(|t| dm.get(*t).and_then(|i| i.class.prop(meta.name)).is_some_and(|rp| rp.meta.ty == meta.ty))
}

/// Whether `meta` is the `Parent` of an object that cannot be moved (a service, or the root).
fn fixed_parent(dm: &DataModel, id: Id, meta: &PropMeta) -> bool {
    meta.name == "Parent" && dm.get(id).is_some_and(|i| i.class.desc.service_order.is_some() || i.class.desc.root)
}

/// The properties of `class` the panel lists, in groups, for what is selected: visible ones whose
/// condition holds for the active object, and (with several objects selected) only the shared ones.
/// The second list holds the names that are hidden because the selection does not all have them.
fn visible_props<'c>(dm: &DataModel, class: &'c ResolvedClass, active: Id, targets: &[Id]) -> (Vec<(&'static str, Vec<&'c ResolvedProp>)>, Vec<&'static str>) {
    let mut groups = Vec::new();
    let mut unavailable = Vec::new();
    for group in class.groups() {
        let mut rows = Vec::new();
        for rp in class.props.iter().filter(|rp| rp.meta.visible && rp.meta.display_group() == group) {
            if rp.meta.visible_if.is_some_and(|cond| !cond.evaluate(dm, active)) {
                continue;
            }
            if targets.len() > 1 && !shared_by_all(dm, targets, &rp.meta) {
                unavailable.push(rp.meta.display_label());
                continue;
            }
            rows.push(rp);
        }
        if !rows.is_empty() {
            groups.push((group, rows));
        }
    }
    (groups, unavailable)
}

/// Draws the Properties panel. Nothing here is class-specific: groups, order, labels, tooltips,
/// visibility, read-only state, conditional visibility and the editor control all come from each
/// property's `PropMeta`.
pub(super) fn properties(ui: &mut egui::Ui, ed: &mut Editor) {
    ed.normalize_selection();
    let Some((active, is_selected)) = ed.inspected() else {
        ui.label("Select an object in the Explorer or click one in the viewport.");
        return;
    };
    // With several objects selected they are all edited together; an object that is only on show
    // (nothing selected) is edited on its own.
    let targets: Vec<Id> = if is_selected { ed.selection_ids() } else { vec![active] };
    let (class, shown) = {
        let dm = ed.dm.borrow();
        let Some(class) = dm.get(active).map(|i| i.class) else { return };
        (class, format!("{} \"{}\"", class.name, dm.name(active)))
    };

    // The class and name of what is shown are in the window's title (see `Editor::properties_title`), so the list
    // starts at the top. Only what the title cannot say is written here.
    if targets.len() > 1 {
        ui.secondary(format!("{} objects selected. Editing a value changes it on every selected object that has the property.", targets.len()));
    } else if !is_selected {
        // The object on show is not the selection (so the title does not name it): say so, so it cannot be mistaken for it.
        ui.colored_label(MIXED, format!("Not selected - showing {shown}, the last selected object."));
    }

    let scripts = ed.scripts.clone();
    let mut open_script: Option<String> = None;
    let mut pick: Option<(&'static str, AssetKind)> = None;
    let mut open_material: Option<String> = None;
    let mut parent_pick = ParentPick::Nothing;
    if ed.picking_parent.is_some() {
        ui.colored_label(ui.visuals().selection.stroke.color, "Pick the new parent: click an object in the Explorer or in the viewport. Esc cancels.");
    } else if ed.picking_primary.is_some() {
        ui.colored_label(ui.visuals().selection.stroke.color, "Pick the primary part: click a part of this model in the Explorer or in the viewport. Esc cancels.");
    }

    let (groups, unavailable) = visible_props(&ed.dm.borrow(), class, active, &targets);
    for (group, rows) in groups {
        egui::CollapsingHeader::new(group).default_open(true).show(ui, |ui| {
            for rp in rows {
                let mut env = EditEnv { scripts: &scripts, open_script: &mut open_script, pick: &mut pick, open_material: &mut open_material, parent_pick: &mut parent_pick };
                property_row(ui, ed, active, &targets, &rp.meta, &mut env);
            }
        });
    }
    if !unavailable.is_empty() {
        egui::CollapsingHeader::new(RichText::new(format!("Not shared ({})", unavailable.len())).color(crate::theme::secondary_color(ui))).default_open(false).show(ui, |ui| {
            ui.secondary("Only some of the selected objects have these, so they cannot be edited together.");
            ui.secondary(unavailable.join(", "));
        });
    }

    if let Some(p) = open_script {
        ed.open_script(&p);
    }
    if let Some(p) = open_material {
        ed.open_material_editor(Some(&p));
    }
    if let Some((prop, kind)) = pick {
        let current = ed.dm.borrow().string(active, prop);
        let title = format!("Choose {} for {}", prop_label(class, prop), if targets.len() > 1 { format!("{} objects", targets.len()) } else { shown.clone() });
        ed.open_picker(PickTarget::Property { targets: targets.clone(), prop }, kind, title, &current);
    }
    match parent_pick {
        ParentPick::Begin => ed.begin_parent_pick(targets.clone()),
        ParentPick::BeginPrimary => ed.begin_primary_pick(active),
        ParentPick::Cancel => ed.cancel_pick(),
        ParentPick::Nothing => {}
    }
}

/// The name the panel gives a property (for the title of the picker).
fn prop_label(class: &ResolvedClass, prop: &str) -> &'static str {
    class.prop(prop).map_or("a file", |rp| rp.meta.display_label())
}

/// Next to the pointer while a parent or a primary part is being chosen: says what a click does, and shows the crosshair.
pub(super) fn parent_pick_hint(ctx: &egui::Context, ed: &Editor) {
    if !ed.picking_any() {
        return;
    }
    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    egui::Area::new(egui::Id::new("forge_parent_pick_hint")).order(egui::Order::Tooltip).fixed_pos(pos + egui::vec2(16.0, 16.0)).interactable(false).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            if ed.picking_primary.is_some() {
                ui.label("Click a part of the model to make it the primary part");
            } else {
                ui.label("Click an object to make it the parent");
            }
        });
    });
}

/// One property: the control for the active object's value, and what editing it does to the others.
fn property_row(ui: &mut egui::Ui, ed: &mut Editor, active: Id, targets: &[Id], meta: &PropMeta, env: &mut EditEnv) {
    let values: Vec<(Id, Value)> = {
        let dm = ed.dm.borrow();
        targets.iter().filter_map(|t| dm.get_prop(*t, meta.name).map(|v| (*t, v))).collect()
    };
    let Some(current) = values.iter().find(|(id, _)| *id == active).map(|(_, v)| v.clone()).or_else(|| values.first().map(|(_, v)| v.clone())) else { return };
    let mixed = values.iter().any(|(_, v)| *v != current);
    // Objects that cannot take the edit (the property is read-only on them) are left alone.
    let writable: Vec<Id> = {
        let dm = ed.dm.borrow();
        targets.iter().copied().filter(|t| dm.get(*t).and_then(|i| i.class.prop(meta.name)).is_some_and(|rp| rp.meta.editable()) && !fixed_parent(&dm, *t, meta)).collect()
    };
    let editable = meta.editable() && !writable.is_empty();
    // A model's natural size, with a button that gives it to the part (the one editor control that is not a plain value).
    if meta.name == "MeshSize" {
        mesh_size_row(ui, ed, targets, &current, meta);
        return;
    }
    let mut value = current.clone();
    let mut make_equal = false;

    let row = ui.horizontal(|ui| {
        ui.label(meta.display_label());
        ui.add_enabled_ui(editable, |ui| edit_value(ui, ed, active, meta, &mut value, env));
        if mixed {
            ui.label(RichText::new("mixed").color(MIXED).italics()).on_hover_text(format!("The {} selected objects have different values. Editing sets all of them to the new value.", targets.len()));
            if editable && ui.small_button("=").on_hover_text("Make every selected object's value the same as the active object's").clicked() {
                make_equal = true;
            }
        }
    });
    if !meta.description.is_empty() {
        row.response.on_hover_text(meta.description);
    }

    if !editable {
        return;
    }
    if make_equal || value != current {
        write_all(ed, &writable, meta, value);
    }
}

/// MeshSize: the size of the model in the file, and a button that makes the part that size.
fn mesh_size_row(ui: &mut egui::Ui, ed: &mut Editor, targets: &[Id], current: &Value, meta: &PropMeta) {
    let Value::V3(size) = current else { return };
    ui.horizontal(|ui| {
        ui.label(meta.display_label());
        ui.label(format!("{:.2} x {:.2} x {:.2}", size.x, size.y, size.z));
        let usable = *size != Vec3::ZERO;
        if ui.add_enabled(usable, egui::Button::new("Use as Size").small()).on_hover_text("Make the part the size of its model, so the model is not stretched").clicked() {
            let mut dm = ed.dm.borrow_mut();
            for t in targets {
                let natural = dm.get_prop(*t, "MeshSize");
                if let Some(Value::V3(s)) = natural {
                    let _ = dm.set_prop(*t, "Size", Value::V3(s));
                }
            }
            drop(dm);
            ed.dirty = true;
        }
    })
    .response
    .on_hover_text(meta.description);
}

/// Writes one value to every object in `targets` (they all allow it), reporting a refusal.
fn write_all(ed: &mut Editor, targets: &[Id], meta: &PropMeta, value: Value) {
    // A file: the same path as dropping or picking one (the first model a part gets gives it the model's size).
    if let (PropType::Asset(_), Value::Str(path)) = (meta.ty, &value) {
        ed.assign_asset_prop(targets, meta.name, path);
        return;
    }
    let mut failure = None;
    // Scripts that are renamed take their file's name with them (see Editor::queue_script_rename).
    let old_names: Vec<(Id, String)> = if meta.name == "Name" { targets.iter().map(|t| (*t, ed.dm.borrow().name(*t))).collect() } else { Vec::new() };
    {
        let mut dm = ed.dm.borrow_mut();
        for target in targets {
            if let Err(e) = dm.set_prop(*target, meta.name, value.clone()) {
                failure = Some(e);
            }
        }
    }
    match failure {
        None => {
            ed.dirty = true;
            for (id, old) in old_names {
                ed.queue_script_rename(id, &old);
            }
        }
        Some(e) => ed.warn(e),
    }
}
/// The editor control for one property, chosen by its type and `Widget` hint.
fn edit_value(ui: &mut egui::Ui, ed: &mut Editor, id: Id, meta: &PropMeta, value: &mut Value, env: &mut EditEnv) {
    match (meta.ty, value) {
        (PropType::Bool, Value::Bool(b)) => {
            ui.checkbox(b, "");
        }
        (PropType::Number, Value::Num(n)) => match meta.widget {
            Widget::Slider { min, max } => {
                ui.add(egui::Slider::new(n, min..=max));
            }
            _ => {
                let speed = match meta.widget {
                    Widget::Drag { speed } => speed as f64,
                    _ => ((n.abs() as f64) * 0.01).max(0.02),
                };
                let mut drag = egui::DragValue::new(n).speed(speed);
                if let Some((lo, hi)) = meta.clamp {
                    drag = drag.range(lo..=hi);
                }
                ui.add(drag);
            }
        },
        (PropType::String, Value::Str(s)) => {
            if meta.widget == Widget::Multiline {
                ui.add(egui::TextEdit::multiline(s).desired_rows(2));
            } else {
                ui.text_edit_singleline(s);
            }
        }
        (PropType::Vector3, Value::V3(v)) => {
            let speed = match meta.widget {
                Widget::Drag { speed } => speed as f64,
                _ => 0.05,
            };
            for (axis, comp) in ["X", "Y", "Z"].into_iter().zip([&mut v.x, &mut v.y, &mut v.z]) {
                ui.label(axis);
                let mut drag = egui::DragValue::new(comp).speed(speed);
                if let Some((lo, hi)) = meta.clamp {
                    drag = drag.range(lo..=hi);
                }
                ui.add(drag);
            }
        }
        (PropType::Vector2, Value::V2(v)) => {
            for (axis, comp) in ["X", "Y"].into_iter().zip([&mut v.x, &mut v.y]) {
                ui.label(axis);
                ui.add(egui::DragValue::new(comp).speed(0.05));
            }
        }
        // A NumberRange, a BrickColor ...: its numbers, in the order the type lists them (see `core::DATA_TYPES`).
        (PropType::Data(tag), Value::Data(_, numbers)) => {
            if numbers.is_empty() {
                ui.secondary(format!("{tag}: none"));
            } else {
                ui.secondary(tag);
                for number in numbers.iter_mut().take(24) {
                    ui.add(egui::DragValue::new(number).speed(0.01));
                }
            }
        }
        (PropType::Color3, Value::Color(c)) => {
            let mut rgb = c.to_array();
            if ui.color_edit_button_rgb(&mut rgb).changed() {
                *c = Vec3::from(rgb);
            }
        }
        (PropType::Enum(t), Value::Enum(e)) => {
            egui::ComboBox::from_id_salt((meta.name, "enum")).selected_text(e.clone()).show_ui(ui, |ui| {
                for option in enum_names(t) {
                    ui.selectable_value(e, (*option).to_owned(), *option);
                }
            });
        }
        // A model's primary part is chosen like a parent (a button, then a click on an object), but only a part
        // inside the model is taken; it can be cleared (the model then has no primary part).
        (PropType::Ref(_), Value::Ref(r)) if meta.name == "PrimaryPart" => {
            let dm = ed.dm.borrow();
            let current = (*r).filter(|x| dm.exists(*x));
            let name = current.map(|x| dm.name(x)).unwrap_or_else(|| "(none)".to_owned());
            let path = current.map(|x| dm.full_name(x));
            drop(dm);
            if ed.picking_primary == Some(id) {
                ui.add(egui::Button::new("Click a part of this model...").selected(true));
                if ui.button("Cancel").clicked() {
                    *env.parent_pick = ParentPick::Cancel;
                }
            } else {
                let hint = match &path {
                    Some(path) => format!("{path}\n\nClick, then click one of the model's parts, in the Explorer or the viewport"),
                    None => "Click, then click one of the model's parts, in the Explorer or the viewport".to_owned(),
                };
                if ui.button(format!("{name}   (change...)")).on_hover_text(hint).clicked() {
                    *env.parent_pick = ParentPick::BeginPrimary;
                }
                if ui.add_enabled(current.is_some(), egui::Button::new("Clear")).on_hover_text("No primary part: the model's pivot is then that of the first part inside it").clicked() {
                    *r = None;
                }
            }
        }
        (PropType::Ref(_), Value::Ref(r)) if meta.name == "Parent" => {
            let name = (*r).and_then(|x| ed.dm.borrow().get(x).map(|i| i.name.clone())).unwrap_or_else(|| "(none)".to_owned());
            if ed.picking_parent.is_some() {
                ui.add(egui::Button::new("Click an object...").selected(true));
                if ui.button("Cancel").clicked() {
                    *env.parent_pick = ParentPick::Cancel;
                }
            } else if ui.button(format!("{name}   (change...)")).on_hover_text("Click, then click the object to move this into, in the Explorer or the viewport").clicked() {
                *env.parent_pick = ParentPick::Begin;
            }
        }
        (PropType::Ref(want), Value::Ref(r)) => {
            let label = (*r).and_then(|x| ed.dm.borrow().get(x).map(|i| i.name.clone())).unwrap_or_else(|| "(none)".to_owned());
            egui::ComboBox::from_id_salt((meta.name, "ref")).selected_text(label).width(160.0).show_ui(ui, |ui| {
                if ui.selectable_label(r.is_none(), "(none)").clicked() {
                    *r = None;
                }
                // A search box, so a big scene's objects are all reachable (only the first
                // `MAX_REF_OPTIONS` matches are listed).
                let filter_id = ui.id().with((meta.name, "ref_filter"));
                let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
                ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search...").desired_width(220.0));
                ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
                let needle = filter.to_lowercase();

                let dm = ed.dm.borrow();
                let mut options: Vec<(String, Id)> = dm
                    .all_ids()
                    .into_iter()
                    .filter(|c| {
                        *c != id
                            && dm.get(*c).is_some_and(|i| {
                                i.class.desc.service_order.is_none() && (want == "Instance" || i.class.is_a(want)) && (needle.is_empty() || i.name.to_lowercase().contains(&needle))
                            })
                    })
                    .map(|c| (dm.full_name(c), c))
                    .collect();
                options.sort_by(|a, b| a.0.cmp(&b.0));
                let total = options.len();
                for (full, c) in options.into_iter().take(MAX_REF_OPTIONS) {
                    if ui.selectable_label(*r == Some(c), full).clicked() {
                        *r = Some(c);
                    }
                }
                if total > MAX_REF_OPTIONS {
                    ui.secondary(format!("... {} more: type to narrow the list", total - MAX_REF_OPTIONS));
                }
            });
        }
        (PropType::ScriptPath, Value::Str(s)) => {
            egui::ComboBox::from_id_salt((meta.name, "script")).selected_text(if s.is_empty() { "(none)".to_owned() } else { s.clone() }).show_ui(ui, |ui| {
                if ui.selectable_label(s.is_empty(), "(none)").clicked() {
                    s.clear();
                }
                for p in env.scripts {
                    ui.selectable_value(s, p.clone(), p);
                }
            });
            if !s.is_empty() && ui.button("Edit").clicked() {
                *env.open_script = Some(s.clone());
            }
        }
        // A file of the project: a box that shows it, takes a file dragged from the Asset Browser, opens the picker when clicked.
        (PropType::Asset(ext), Value::Str(s)) => {
            let kind = Editor::kind_of_asset_ext(ext);
            let current = s.clone();
            match asset_field(ui, ed, meta.name, kind, &current, kind == AssetKind::Material) {
                FieldAction::Set(p) => *s = p,
                FieldAction::Clear => s.clear(),
                FieldAction::Pick => *env.pick = Some((meta.name, kind)),
                FieldAction::Edit => *env.open_material = Some(current),
                FieldAction::None => {}
            }
        }
        _ => {
            ui.secondary("(not editable)");
        }
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
            let dir = std::env::temp_dir().join(format!("forge_inspector_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Inspector Test").expect("create project");
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

    fn draw(ctx: &egui::Context, ed: &mut Editor) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 900.0))), ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| {
            egui::CentralPanel::default().show(&mut forge_engine::compat::root_ui(ctx), |ui| properties(ui, ed));
        });
    }

    /// Every piece of text one frame of `properties` puts on the screen, top to bottom.
    fn texts(ctx: &egui::Context, ed: &mut Editor) -> Vec<String> {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 900.0))), ..Default::default() };
        let out = ctx.run_frame(raw, |ctx| {
            egui::CentralPanel::default().show(&mut forge_engine::compat::root_ui(ctx), |ui| properties(ui, ed));
        });
        let mut found: Vec<(f32, String)> = out.shapes.iter().filter_map(|s| if let egui::epaint::Shape::Text(t) = &s.shape { Some((t.pos.y, t.galley.text().to_owned())) } else { None }).collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|(_, t)| t).collect()
    }

    #[test]
    fn the_panel_starts_with_the_property_list_the_class_and_name_are_in_the_window_title() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (wall, step) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        let description = ed.dm.borrow().get(wall).unwrap().class.desc.description;
        assert!(!description.is_empty(), "the class does have a description (otherwise this proves nothing)");
        ed.select_only(Some(wall));
        let ctx = egui::Context::default();
        texts(&ctx, &mut ed);
        let shown = texts(&ctx, &mut ed);
        assert!(shown.iter().all(|t| !t.contains(description)), "the class description is gone: {shown:?}");
        assert_eq!(shown.first().map(String::as_str), Some("General"), "the list starts at the top, with its first group: {shown:?}");
        assert!(shown.iter().all(|t| t != "Part" && !t.contains("Workspace.Wall")), "no class heading or path above it: {shown:?}");
        assert!(shown.iter().any(|t| t == "Size" || t.contains("Size")), "and the properties are still there");
        assert!(ed.properties_title().starts_with("Properties - Part \"Wall\""), "what the heading said is in the title now");

        // Several objects: one line about editing them together, then the list.
        ed.select_toggle(step);
        let shown = texts(&ctx, &mut ed);
        assert!(shown.first().is_some_and(|t| t.starts_with("2 objects selected")), "{shown:?}");
        assert!(shown.iter().any(|t| t == "General"), "{shown:?}");

        // Nothing selected but the last object still on show: the panel says which one it is, as the title does not.
        ed.select_only(None);
        ed.select_only(Some(wall));
        ed.selected = None;
        ed.normalize_selection();
        let shown = texts(&ctx, &mut ed);
        assert!(shown.first().is_some_and(|t| t.starts_with("Not selected - showing Part \"Wall\"")), "{shown:?}");
        assert_eq!(ed.properties_title(), "Properties");
    }

    #[test]
    fn only_properties_every_selected_object_has_are_offered_together() {
        let s = Scratch::new();
        let ed = editor(&s);
        let part = find(&ed, "Wall");
        let light = {
            let mut dm = ed.dm.borrow_mut();
            let l = dm.create("PointLight").unwrap();
            let ws = dm.workspace;
            dm.set_parent(l, Some(ws)).unwrap();
            l
        };
        let dm = ed.dm.borrow();
        let class = dm.get(part).unwrap().class;
        let names = |targets: &[Id]| -> (Vec<&str>, Vec<&str>) {
            let (groups, unavailable) = visible_props(&dm, class, part, targets);
            (groups.iter().flat_map(|(_, rows)| rows.iter().map(|r| r.meta.name)).collect(), unavailable)
        };
        let (alone, none_hidden) = names(&[part]);
        assert!(alone.contains(&"Size") && alone.contains(&"Color") && none_hidden.is_empty());

        let (together, hidden) = names(&[part, light]);
        assert!(together.contains(&"Color"), "both have a colour of the same type");
        assert!(together.contains(&"Name"));
        assert!(!together.contains(&"Size"), "a light has no size");
        assert!(hidden.contains(&"Size"), "and the panel says it left Size out");
    }

    #[test]
    fn parent_is_listed_under_general_and_services_keep_theirs() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (wall, model) = (find(&ed, "Wall"), find(&ed, "Player"));
        let meta = ed.dm.borrow().get(wall).unwrap().class.prop("Parent").unwrap().meta;
        {
            let dm = ed.dm.borrow();
            let (groups, _) = visible_props(&dm, dm.get(wall).unwrap().class, wall, &[wall]);
            let (name, rows) = &groups[0];
            assert_eq!(*name, "General");
            assert_eq!(rows.iter().map(|r| r.meta.name).collect::<Vec<_>>(), ["Name", "Parent"]);
            assert!(!fixed_parent(&dm, wall, &meta));
            assert!(fixed_parent(&dm, dm.workspace, &meta), "a service cannot be moved");
        }
        write_all(&mut ed, &[wall], &meta, Value::Ref(Some(model)));
        assert_eq!(ed.dm.borrow().parent(wall), Some(model));
        let ws = ed.dm.borrow().workspace;
        write_all(&mut ed, &[wall], &meta, Value::Ref(Some(ws)));
        assert_eq!(ed.dm.borrow().parent(wall), Some(ws), "a service is a valid new parent");
    }

    #[test]
    fn the_parent_is_chosen_by_clicking_an_object_not_from_a_dropdown() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (wall, model, root) = (find(&ed, "Wall"), find(&ed, "Player"), {
            let dm = ed.dm.borrow();
            let m = dm.find_first_child(dm.workspace, "Player", true).unwrap();
            dm.find_child(m, "Body").unwrap()
        });
        let ctx = egui::Context::default();
        ed.select_only(Some(wall));
        draw(&ctx, &mut ed);
        assert!(ed.picking_parent.is_none(), "just showing the panel starts nothing");

        // Clicking the Parent control starts the choosing; the next object clicked becomes the parent.
        ed.begin_parent_pick(vec![wall]);
        draw(&ctx, &mut ed);
        assert_eq!(ed.picking_parent, Some(vec![wall]));
        assert!(ed.parent_pick_ok(model) && !ed.parent_pick_ok(wall), "an object cannot be its own parent");
        assert!(ed.pick_parent(model));
        assert_eq!(ed.dm.borrow().parent(wall), Some(model));
        assert!(ed.picking_parent.is_none() && ed.dirty);
        assert_eq!(ed.selection, vec![wall], "the selection is not disturbed");

        // Inside itself is refused, and the choosing goes on until Esc or a good pick.
        ed.begin_parent_pick(vec![model]);
        assert!(!ed.parent_pick_ok(root) && !ed.pick_parent(root));
        assert!(ed.picking_parent.is_some(), "a bad pick does not end the choosing");
        assert_eq!(ed.dm.borrow().parent(model), Some(ed.dm.borrow().workspace));
        ed.cancel_parent_pick();
        assert!(ed.picking_parent.is_none());

        // Services can be the parent, and cannot be moved.
        let storage = ed.dm.borrow().service("ReplicatedStorage").unwrap();
        ed.begin_parent_pick(vec![wall]);
        assert!(ed.pick_parent(storage));
        assert_eq!(ed.dm.borrow().parent(wall), Some(storage));
        ed.begin_parent_pick(vec![storage]);
        assert!(ed.picking_parent.is_none(), "a service has nothing to pick a parent for");

        // Several objects at once, and picking the parent they already have changes nothing.
        let (a, b) = (find(&ed, "Step 1"), find(&ed, "Step 2"));
        ed.begin_parent_pick(vec![a, b]);
        assert!(ed.pick_parent(model));
        assert_eq!((ed.dm.borrow().parent(a), ed.dm.borrow().parent(b)), (Some(model), Some(model)));
        ed.dirty = false;
        ed.begin_parent_pick(vec![a, b]);
        assert!(ed.pick_parent(model));
        assert!(!ed.dirty, "nothing moved");
        // One undo step takes a move back.
        ed.commit_edits(false);
        ed.begin_parent_pick(vec![a]);
        ed.pick_parent(storage);
        ed.commit_edits(false);
        ed.undo();
        assert_eq!(ed.dm.borrow().parent(a), Some(model));
    }

    #[test]
    fn an_edit_reaches_every_selected_object_and_nothing_else() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b, bystander) = (find(&ed, "Wall"), find(&ed, "Step 1"), find(&ed, "Step 2"));
        let meta = ed.dm.borrow().get(a).unwrap().class.prop("Transparency").unwrap().meta;

        write_all(&mut ed, &[a, b], &meta, Value::Num(0.5));
        assert_eq!(ed.dm.borrow().num(a, "Transparency", -1.0), 0.5);
        assert_eq!(ed.dm.borrow().num(b, "Transparency", -1.0), 0.5);
        assert_eq!(ed.dm.borrow().num(bystander, "Transparency", -1.0), 0.0, "unselected objects are untouched");
        assert!(ed.dirty);

        // Each object keeps its own unrelated properties.
        assert_ne!(ed.dm.borrow().size(a), ed.dm.borrow().size(b));
        // The whole edit is one undo step.
        ed.commit_edits(false);
        ed.undo();
        assert_eq!(ed.dm.borrow().num(a, "Transparency", -1.0), 0.0);
        assert_eq!(ed.dm.borrow().num(b, "Transparency", -1.0), 0.0);
    }

    #[test]
    fn the_panel_keeps_the_last_object_when_the_selection_is_cleared() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let wall = find(&ed, "Wall");
        let ctx = egui::Context::default();

        ed.select_only(Some(wall));
        draw(&ctx, &mut ed);
        assert_eq!(ed.inspected(), Some((wall, true)));

        ed.select_only(None);
        draw(&ctx, &mut ed);
        assert!(ed.selected.is_none());
        assert_eq!(ed.inspected(), Some((wall, false)), "still on show, but marked as not selected");

        // Several objects, then the selection is cleared again.
        let step = find(&ed, "Step 1");
        ed.select_only(Some(wall));
        ed.select_toggle(step);
        draw(&ctx, &mut ed);
        assert_eq!(ed.inspected(), Some((step, true)), "the active object leads");
        ed.select_only(None);
        draw(&ctx, &mut ed);
        assert_eq!(ed.inspected(), Some((step, false)));
    }

    #[test]
    fn deleting_what_is_on_show_falls_back_to_its_parent_without_selecting_it() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let model = find(&ed, "Player");
        let root = ed.dm.borrow().find_child(model, "Body").unwrap();
        let ctx = egui::Context::default();

        ed.select_only(Some(root));
        draw(&ctx, &mut ed);
        ed.delete_selected();
        draw(&ctx, &mut ed);
        assert!(ed.selected.is_none() && ed.selection.is_empty());
        assert_eq!(ed.inspected(), Some((model, false)), "the panel moves to the part's model, marked as not selected");

        ed.delete(model);
        draw(&ctx, &mut ed);
        let shown = ed.inspected().map(|(id, _)| ed.dm.borrow().class_name(id));
        assert_ne!(shown, Some("Workspace"), "never falls back to a service");

        // A panel pointing at something that vanished (say, by undo) must not break.
        ed.last_inspected = Some(root);
        draw(&ctx, &mut ed);
        assert_eq!(ed.inspected(), None);
    }

    #[test]
    fn a_multi_selection_draws() {
        let s = Scratch::new();
        let mut ed = editor(&s);
        let (a, b) = (find(&ed, "Wall"), find(&ed, "Step 1"));
        ed.select_only(Some(a));
        ed.select_toggle(b);
        let ctx = egui::Context::default();
        draw(&ctx, &mut ed);
        draw(&ctx, &mut ed);
    }
}