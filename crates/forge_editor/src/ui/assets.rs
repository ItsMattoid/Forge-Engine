//! The Asset Browser (the Assets panel).
//!
//! * left: the project's real folders as a tree;
//! * top: the breadcrumb of the folder being browsed, the New / Import menus and the search box;
//! * main: the contents of that folder as a grid of tiles (folders first), or, while searching, the
//!   matches in that folder and the folders below it.
//!
//! Everything it does goes through the editor's file system (`state::assets`): the list comes from the
//! scan of the real folders, and renames, moves, deletes and imports are real file operations that keep
//! the scenes, the script editor and the Explorer pointing at the right files.

use std::{path::Path, time::SystemTime};

use bevy_egui::egui::{self, vec2, Align2, Color32, FontId, Rect, RichText, Sense};
use forge_engine::project::{file_name, join, parent_of, AssetKind, SearchHit, SearchQuery};

use crate::{
    state::{AssetDrag, Dialog, Editor, Thumb},
    theme::{secondary_color, SecondaryText},
};

/// Thumbnails read per frame (decoding an image costs a moment).
const THUMBS_PER_FRAME: u32 = 3;
/// Images larger than this many bytes are not read for a thumbnail.
const MAX_THUMB_BYTES: u64 = 24 * 1024 * 1024;
const THUMB_EDGE: u32 = 128;
const PREVIEW_EDGE: u32 = 768;
const HINT_KEY: &str = "forge_asset_drag_hint";

/// What the pointer would do with the dragged assets where it is now (shown next to the pointer).
pub(super) fn set_drag_hint(ctx: &egui::Context, ok: bool, text: String) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(HINT_KEY), (ok, text)));
}

/// Draws the drag feedback next to the pointer while assets are being dragged, then forgets it.
pub(super) fn drag_hint(ctx: &egui::Context) {
    let id = egui::Id::new(HINT_KEY);
    let hint: Option<(bool, String)> = ctx.data_mut(|d| d.remove_temp(id));
    let Some(drag) = egui::DragAndDrop::payload::<AssetDrag>(ctx) else { return };
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let (ok, text) = hint.unwrap_or_else(|| (true, if drag.paths.len() == 1 { file_name(&drag.paths[0]).to_owned() } else { format!("{} items", drag.paths.len()) }));
    let color = if ok { Color32::from_rgb(120, 200, 120) } else { Color32::from_rgb(235, 100, 100) };
    egui::Area::new(egui::Id::new("forge_asset_drag_hint_area")).order(egui::Order::Tooltip).fixed_pos(pos + vec2(14.0, 14.0)).interactable(false).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(RichText::new(format!("{} {text}", if ok { "+" } else { "x" })).color(color));
        });
    });
}

/// The tint drawn over a drop target while assets hover it: green when the drop works, red when not.
pub(super) fn drop_tint(ui: &egui::Ui, rect: Rect, ok: bool) {
    let color = if ok { Color32::from_rgba_unmultiplied(120, 200, 120, 60) } else { Color32::from_rgba_unmultiplied(235, 100, 100, 60) };
    ui.painter().rect_filled(rect, 3.0, color);
}

// ---- the tab ---------------------------------------------------------------------------------------

pub(super) fn assets_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    if ed.project.is_none() {
        ui.label("No project open.");
        return;
    }
    ed.sync_assets(false);
    if ed.asset_db().is_none() {
        ui.label("Reading the project's files...");
        return;
    }
    header(ui, ed);
    ui.separator();
    egui::Panel::left("asset_folders").resizable(true).default_size(190.0).size_range(110.0..=420.0).show(ui, |ui| folder_tree(ui, ed));
    egui::CentralPanel::default().show(ui, |ui| grid(ui, ed));
}

fn header(ui: &mut egui::Ui, ed: &mut Editor) {
    // The breadcrumb: Project > Assets > Models > Characters. Each part goes to that folder.
    ui.horizontal_wrapped(|ui| {
        let project_name = ed.project.as_ref().map(|p| p.name().to_owned()).unwrap_or_default();
        let dir = ed.assets_ui.dir.clone();
        let mut parts: Vec<(String, String)> = vec![(project_name, String::new())];
        let mut acc = String::new();
        for part in dir.split('/').filter(|p| !p.is_empty()) {
            acc = join(&acc, part);
            parts.push((part.to_owned(), acc.clone()));
        }
        let last = parts.len() - 1;
        for (i, (label, path)) in parts.into_iter().enumerate() {
            let text = if i == last { RichText::new(label).strong() } else { RichText::new(label) };
            let resp = ui.add(egui::Button::new(text).frame(false));
            if resp.clicked() {
                ed.assets_ui.search.clear();
                ed.browse_to(&path);
            }
            if let Some(drag) = resp.dnd_hover_payload::<AssetDrag>() {
                drop_tint(ui, resp.rect, can_move_into(&drag.paths, &path));
            }
            if let Some(drag) = resp.dnd_release_payload::<AssetDrag>() {
                ed.asset_move(&drag.paths, &path);
            }
            if i != last {
                ui.secondary(">");
            }
        }
    });

    ui.horizontal(|ui| {
        ui.menu_button("+ New", |ui| {
            if ui.button("Folder").clicked() {
                ed.asset_new_folder();
                ui.close();
            }
            ui.separator();
            for (label, class) in [("Luau Script", "Script"), ("Local Script", "LocalScript"), ("Module Script", "ModuleScript")] {
                if ui.button(label).on_hover_text("An empty Luau file. Add it to the scene by dragging it onto an object.").clicked() {
                    ed.asset_new_script(class);
                    ui.close();
                }
            }
            ui.separator();
            ui.menu_button("Material", |ui| {
                for kind in forge_engine::assets::material::MaterialKind::ALL {
                    if ui.button(kind.label()).on_hover_text(kind.description()).clicked() {
                        ed.new_material(kind);
                        ui.close();
                    }
                }
            });
            if ui.button("Scene...").clicked() {
                ed.dialog = Dialog::NewScene { name: "Level".into() };
                ui.close();
            }
        });
        ui.menu_button("Import", |ui| {
            if ui.button("Files...").on_hover_text("Copy files into the folder being browsed").clicked() {
                if let Some(files) = rfd::FileDialog::new().set_title("Import files").pick_files() {
                    ed.asset_import(files);
                }
                ui.close();
            }
            if ui.button("Folder...").clicked() {
                if let Some(dir) = rfd::FileDialog::new().set_title("Import a folder").pick_folder() {
                    ed.asset_import(vec![dir]);
                }
                ui.close();
            }
        });
        let width = (ui.available_width() - 190.0).max(80.0);
        ui.add(egui::TextEdit::singleline(&mut ed.assets_ui.search).hint_text("Search by name  (type:mesh  ext:png)").desired_width(width));
        if ui.add_enabled(!ed.assets_ui.search.is_empty(), egui::Button::new("x").small()).on_hover_text("Clear the search").clicked() {
            ed.assets_ui.search.clear();
        }
        ui.add(egui::Slider::new(&mut ed.assets_ui.tile, 56.0..=140.0).show_value(false)).on_hover_text("Tile size");
    });
}

/// Whether the dragged items can be moved into the folder `dest` (not into themselves, not where they already are).
fn can_move_into(paths: &[String], dest: &str) -> bool {
    !paths.is_empty() && paths.iter().all(|p| p != dest && !dest.starts_with(&format!("{p}/")) && parent_of(p) != dest)
}

// ---- the folder tree -------------------------------------------------------------------------------

/// The folders of the project. The project folder itself is not a row here: it is the first part of the
/// breadcrumb above (click it to go back to the top), so the tree starts with the folders inside it.
fn folder_tree(ui: &mut egui::Ui, ed: &mut Editor) {
    let top: Vec<String> = ed.asset_db().map(|db| db.contents("").0.to_vec()).unwrap_or_default();
    egui::ScrollArea::both().id_salt("asset_folder_tree").auto_shrink([false, false]).show(ui, |ui| {
        for folder in top {
            tree_row(ui, ed, &folder, 0);
        }
    });
}

fn tree_row(ui: &mut egui::Ui, ed: &mut Editor, path: &str, depth: usize) {
    let subfolders: Vec<String> = ed.asset_db().map(|db| db.contents(path).0.to_vec()).unwrap_or_default();
    let open = path.is_empty() || ed.assets_ui.open_folders.contains(path);
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 14.0);
        if !subfolders.is_empty() {
            if ui.small_button(if open { "v" } else { ">" }).clicked() && !ed.assets_ui.open_folders.remove(path) {
                ed.assets_ui.open_folders.insert(path.to_owned());
            }
        } else {
            ui.add_space(20.0);
        }
        let label = if path.is_empty() { ed.project.as_ref().map(|p| p.name().to_owned()).unwrap_or_default() } else { file_name(path).to_owned() };
        let current = ed.assets_ui.dir == path;
        let mut text = RichText::new(label);
        if current {
            text = text.background_color(ui.visuals().selection.bg_fill).color(ui.visuals().selection.stroke.color);
        }
        let resp = ui.add(egui::Label::new(text).selectable(false).sense(Sense::click_and_drag()));
        if resp.clicked() {
            ed.assets_ui.search.clear();
            ed.browse_to(path);
        }
        if !path.is_empty() {
            resp.dnd_set_drag_payload(AssetDrag { paths: vec![path.to_owned()] });
        }
        if let Some(drag) = resp.dnd_hover_payload::<AssetDrag>() {
            let ok = can_move_into(&drag.paths, path);
            drop_tint(ui, resp.rect, ok);
            set_drag_hint(ui.ctx(), ok, if ok { format!("Move into {}", if path.is_empty() { "the project folder" } else { file_name(path) }) } else { "Cannot move here".to_owned() });
        }
        if let Some(drag) = resp.dnd_release_payload::<AssetDrag>() {
            ed.asset_move(&drag.paths, path);
        }
        resp.context_menu(|ui| {
            if ui.button("New Folder").clicked() {
                ed.browse_to(path);
                ed.asset_new_folder();
                ui.close();
            }
            if !path.is_empty() {
                if ui.button("Rename").clicked() {
                    ed.browse_to(parent_of(path));
                    ed.start_asset_rename(path);
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    ed.asset_ask_delete(vec![path.to_owned()]);
                    ui.close();
                }
            }
        });
    });
    if open {
        for sub in subfolders {
            tree_row(ui, ed, &sub, depth + 1);
        }
    }
}

// ---- the grid --------------------------------------------------------------------------------------

/// One tile.
struct Item {
    path: String,
    name: String,
    kind: AssetKind,
    size: u64,
    modified: Option<SystemTime>,
}

fn items_of(ed: &Editor) -> (Vec<Item>, Option<usize>) {
    let Some(db) = ed.asset_db() else { return (Vec::new(), None) };
    let dir = ed.assets_ui.dir.as_str();
    let query = SearchQuery::parse(&ed.assets_ui.search);
    let folder = |p: &String| Item { path: p.clone(), name: file_name(p).to_owned(), kind: AssetKind::Folder, size: 0, modified: None };
    let asset = |e: &forge_engine::project::AssetEntry| Item { path: e.path.clone(), name: e.name().to_owned(), kind: e.kind, size: e.size, modified: e.modified };
    if query.is_empty() {
        let (folders, files) = db.contents(dir);
        let items = folders.iter().map(folder).chain(files.into_iter().map(asset)).collect();
        return (items, None);
    }
    let hits = db.search(dir, &query);
    let n = hits.len();
    let items = hits
        .into_iter()
        .filter_map(|h| match h {
            SearchHit::Folder(p) => Some(folder(&p)),
            SearchHit::Asset(p) => db.entry(&p).map(asset),
        })
        .collect();
    (items, Some(n))
}

fn grid(ui: &mut egui::Ui, ed: &mut Editor) {
    let (items, searching) = items_of(ed);
    let order: Vec<String> = items.iter().map(|i| i.path.clone()).collect();
    if let Some(n) = searching {
        ui.horizontal(|ui| {
            let scope = if ed.assets_ui.dir.is_empty() { "the whole project".to_owned() } else { format!("{} and the folders in it", ed.assets_ui.dir) };
            ui.secondary(format!("{n} result{} in {scope}", if n == 1 { "" } else { "s" }));
        });
    }

    // The empty space behind the tiles: click to deselect, right-click for the New menu. (Made before
    // the tiles so that the tiles are on top of it.)
    let area = ui.available_rect_before_wrap();
    let background = ui.interact(area, ui.id().with("asset_background"), Sense::click());
    if background.clicked() {
        ed.assets_ui.selected.clear();
    }
    background.context_menu(|ui| {
        if ui.button("New Folder").clicked() {
            ed.asset_new_folder();
            ui.close();
        }
        if ui.button("New Luau Script").clicked() {
            ed.asset_new_script("Script");
            ui.close();
        }
        if ui.button("New Module Script").clicked() {
            ed.asset_new_script("ModuleScript");
            ui.close();
        }
    });
    if let Some(drag) = egui::DragAndDrop::payload::<AssetDrag>(ui.ctx()) {
        if background.contains_pointer() && searching.is_none() {
            let dir = ed.assets_ui.dir.clone();
            let ok = can_move_into(&drag.paths, &dir);
            drop_tint(ui, area, ok);
            set_drag_hint(ui.ctx(), ok, if ok { format!("Move into {}", if dir.is_empty() { "the project folder" } else { file_name(&dir) }) } else { "Already in this folder".to_owned() });
            if ui.ctx().input(|i| i.pointer.any_released()) && ok {
                ed.asset_move(&drag.paths, &dir);
            }
        }
    }

    if items.is_empty() {
        ui.add_space(8.0);
        ui.secondary(if searching.is_some() { "Nothing matches." } else { "This folder is empty. Drop files here, or use + New and Import." });
        return;
    }

    // Keys, while the pointer is over the grid and no text box has the keyboard.
    if background.contains_pointer() && !ui.ctx().egui_wants_keyboard_input() {
        let (f2, delete) = ui.input(|i| (i.key_pressed(egui::Key::F2), i.key_pressed(egui::Key::Delete)));
        if f2 && ed.assets_ui.selected.len() == 1 {
            let path = ed.assets_ui.selected[0].clone();
            ed.start_asset_rename(&path);
        }
        if delete && !ed.assets_ui.selected.is_empty() {
            let paths = ed.assets_ui.selected.clone();
            ed.asset_ask_delete(paths);
        }
    }

    let tile = ed.assets_ui.tile;
    let (w, h, gap) = (tile, tile + 24.0, 8.0);
    let columns = (((ui.available_width() + gap) / (w + gap)).floor() as usize).max(1);
    let rows = items.len().div_ceil(columns);
    let mut budget = THUMBS_PER_FRAME;
    egui::ScrollArea::vertical().id_salt("asset_grid").auto_shrink([false, false]).show_rows(ui, h + gap, rows, |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for item in items.iter().skip(row * columns).take(columns) {
                    tile_widget(ui, ed, item, &order, vec2(w, h), searching.is_some(), &mut budget);
                }
            });
        }
    });
}

fn tile_widget(ui: &mut egui::Ui, ed: &mut Editor, item: &Item, order: &[String], size: egui::Vec2, in_search: bool, budget: &mut u32) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let selected = ed.assets_ui.selected.contains(&item.path);
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 4.0, ui.visuals().selection.bg_fill);
    } else if resp.hovered() {
        painter.rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let icon = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(size.x - 12.0, size.x - 12.0));
    paint_icon(ui, ed, &painter, icon, item, budget);

    // The name below the picture (or the box it is being renamed in).
    let name_rect = Rect::from_min_max(egui::pos2(rect.min.x + 2.0, icon.max.y + 3.0), rect.max);
    let renaming = ed.assets_ui.renaming.as_ref().is_some_and(|(p, _)| *p == item.path);
    if renaming {
        let mut done: Option<bool> = None;
        if let Some((_, typed)) = ed.assets_ui.renaming.as_mut() {
            let edit = ui.put(name_rect, egui::TextEdit::singleline(typed).desired_width(size.x - 4.0).font(FontId::proportional(12.0)));
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            if edit.lost_focus() {
                done = Some(!ui.input(|i| i.key_pressed(egui::Key::Escape)));
            }
        }
        if let Some(accept) = done {
            ed.finish_asset_rename(accept);
        }
    } else {
        let text_color = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
        let chars = ((size.x - 4.0) / 6.3) as usize;
        painter.with_clip_rect(name_rect).text(egui::pos2(rect.center().x, name_rect.min.y), Align2::CENTER_TOP, shorten(&item.name, chars.max(6)), FontId::proportional(12.0), text_color);
    }

    // Clicking and opening.
    if resp.clicked() {
        let m = ui.input(|i| i.modifiers);
        ed.click_asset(order, &item.path, m.ctrl || m.command, m.shift);
    }
    if resp.double_clicked() {
        ed.open_asset(&item.path);
    }
    if resp.secondary_clicked() && !selected {
        ed.assets_ui.selected = vec![item.path.clone()];
        ed.assets_ui.anchor = Some(item.path.clone());
    }

    // Dragging out (to the scene, the Explorer or another folder) and dropping on a folder.
    if resp.drag_started() && !selected {
        ed.assets_ui.selected = vec![item.path.clone()];
    }
    let carried = if selected { ed.assets_ui.selected.clone() } else { vec![item.path.clone()] };
    resp.dnd_set_drag_payload(AssetDrag { paths: carried });
    if item.kind == AssetKind::Folder {
        if let Some(drag) = resp.dnd_hover_payload::<AssetDrag>() {
            let ok = can_move_into(&drag.paths, &item.path);
            drop_tint(ui, rect, ok);
            set_drag_hint(ui.ctx(), ok, if ok { format!("Move into {}", item.name) } else { "Cannot move here".to_owned() });
        }
        if let Some(drag) = resp.dnd_release_payload::<AssetDrag>() {
            ed.asset_move(&drag.paths, &item.path);
        }
    }

    let resp = resp.on_hover_ui(|ui| tooltip(ui, ed, item, in_search));
    resp.context_menu(|ui| item_menu(ui, ed, item));
}

fn tooltip(ui: &mut egui::Ui, ed: &Editor, item: &Item, in_search: bool) {
    ui.strong(&item.name);
    ui.secondary(item.kind.label());
    if in_search || item.path != item.name {
        ui.secondary(&item.path);
    }
    if item.kind != AssetKind::Folder {
        ui.secondary(format!("{}", format_size(item.size)));
        if !AssetKind::usable_by_engine(&item.name) && matches!(item.kind, AssetKind::Mesh | AssetKind::Texture | AssetKind::Audio) {
            ui.secondary(AssetKind::unusable_reason(&item.name));
        }
        if item.kind == AssetKind::Audio {
            if let Some(seconds) = ed.dm.borrow().assets().and_then(|a| a.audio_length(&item.path)) {
                ui.secondary(format!("{seconds:.2} seconds"));
            }
        }
        if item.kind == AssetKind::Mesh {
            if let Some(size) = ed.dm.borrow().assets().and_then(|a| a.mesh_size(&item.path)) {
                ui.secondary(format!("{:.2} x {:.2} x {:.2} studs", size.x, size.y, size.z));
            }
        }
    }
    let Some(db) = ed.asset_db() else { return };
    if let Some(entry) = db.entry(&item.path) {
        ui.secondary(format!("id {}", entry.id));
    }
    let users = db.used_by(&item.path);
    if !users.is_empty() {
        ui.separator();
        ui.secondary(format!("Used by: {}", users.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
    }
    let deps = db.dependencies_of(&item.path);
    if !deps.is_empty() {
        ui.secondary(format!("Uses {} file(s)", deps.len()));
    }
}

fn item_menu(ui: &mut egui::Ui, ed: &mut Editor, item: &Item) {
    // The menu acts on the whole selection when the item is part of a multi-selection.
    let targets: Vec<String> = if ed.assets_ui.selected.len() > 1 && ed.assets_ui.selected.contains(&item.path) { ed.assets_ui.selected.clone() } else { vec![item.path.clone()] };
    let single = targets.len() == 1;
    if single {
        let open_label = match item.kind {
            AssetKind::Folder => "Open",
            AssetKind::Scene => "Open Scene",
            AssetKind::Script => "Open in Script Editor",
            AssetKind::Texture => "Preview",
            AssetKind::Prefab => "Place in Scene",
            AssetKind::Mesh => "Create Object",
            AssetKind::Material => "Edit in the Material Editor",
            AssetKind::Audio => "Play / Stop",
            _ => "Open",
        };
        if ui.button(open_label).clicked() {
            ed.open_asset(&item.path);
            ui.close();
        }
        if item.kind == AssetKind::Scene && ui.button("Set as Start Scene").clicked() {
            if let Some(p) = ed.project.as_mut() {
                p.manifest.project.start_scene = item.path.clone();
                ed.dirty = true;
                ed.info(format!("{} is now the start scene.", item.path));
            }
            ui.close();
        }
        ui.separator();
        if ui.button("Rename").clicked() {
            ed.start_asset_rename(&item.path);
            ui.close();
        }
    }
    if ui.button("Duplicate").clicked() {
        ed.asset_duplicate(&targets);
        ui.close();
    }
    ui.menu_button("Move to", |ui| {
        let folders: Vec<String> = ed.asset_db().map(|db| db.folders().to_vec()).unwrap_or_default();
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            let project_name = ed.project.as_ref().map(|p| p.name().to_owned()).unwrap_or_default();
            let mut candidates: Vec<(String, String)> = vec![(project_name, String::new())];
            candidates.extend(folders.into_iter().map(|f| (f.clone(), f)));
            for (label, dest) in candidates {
                if !can_move_into(&targets, &dest) {
                    continue;
                }
                if ui.button(label).clicked() {
                    ed.asset_move(&targets, &dest);
                    ui.close();
                }
            }
        });
    });
    if ui.button("Delete").clicked() {
        ed.asset_ask_delete(targets.clone());
        ui.close();
    }
    ui.separator();
    if ui.button("Reveal in Containing Folder").clicked() {
        ed.reveal_asset(&item.path);
        ui.close();
    }
    if single && !matches!(item.kind, AssetKind::Folder | AssetKind::Scene | AssetKind::Script | AssetKind::Prefab | AssetKind::Data) && ui.button("Reimport").on_hover_text("Read the file again (for a mesh, objects using it show the new shape)").clicked() {
        ed.asset_reimport(&item.path);
        ui.close();
    }
}

fn shorten(name: &str, max_chars: usize) -> String {
    let count = name.chars().count();
    if count <= max_chars {
        return name.to_owned();
    }
    // Keep the end (the extension) and the start: "Long Name Of A Fi...e.obj".
    let ext_len = name.rfind('.').map_or(0, |i| name[i..].chars().count()).min(max_chars / 2);
    let head = max_chars.saturating_sub(ext_len + 1);
    let head: String = name.chars().take(head).collect();
    let tail: String = name.chars().skip(count - ext_len).collect();
    format!("{head}~{tail}")
}

fn format_size(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

// ---- icons and thumbnails --------------------------------------------------------------------------

pub(super) fn kind_style(kind: AssetKind) -> (&'static str, Color32) {
    match kind {
        AssetKind::Folder => ("", Color32::from_rgb(220, 170, 70)),
        AssetKind::Scene => ("SCN", Color32::from_rgb(110, 160, 220)),
        AssetKind::Prefab => ("PRE", Color32::from_rgb(130, 190, 230)),
        AssetKind::Script => ("LUA", Color32::from_rgb(90, 130, 230)),
        AssetKind::Mesh => ("3D", Color32::from_rgb(150, 190, 110)),
        AssetKind::Texture => ("IMG", Color32::from_rgb(200, 120, 170)),
        AssetKind::Material => ("MAT", Color32::from_rgb(190, 140, 200)),
        AssetKind::Audio => ("SND", Color32::from_rgb(220, 140, 90)),
        AssetKind::Animation => ("ANI", Color32::from_rgb(120, 200, 190)),
        AssetKind::Font => ("Aa", Color32::from_rgb(180, 180, 120)),
        AssetKind::Data => ("{}", Color32::from_rgb(150, 150, 160)),
        AssetKind::Other => ("?", Color32::from_rgb(130, 130, 140)),
    }
}

fn paint_icon(ui: &egui::Ui, ed: &mut Editor, painter: &egui::Painter, rect: Rect, item: &Item, budget: &mut u32) {
    if item.kind == AssetKind::Texture && AssetKind::has_thumbnail(&item.name) {
        if let Some(texture) = thumbnail(ui.ctx(), ed, &item.path, item.size, item.modified, THUMB_EDGE, budget) {
            // Fit the picture inside the square, keeping its proportions.
            let tex = texture.size_vec2();
            let scale = (rect.width() / tex.x).min(rect.height() / tex.y);
            let fitted = Rect::from_center_size(rect.center(), tex * scale);
            painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
            painter.image(texture.id(), fitted, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            return;
        }
    }
    if item.kind == AssetKind::Material {
        if let Some(texture) = material_thumb(ui.ctx(), ed, &item.path, item.size, item.modified, budget) {
            let side = rect.width().min(rect.height());
            let fitted = Rect::from_center_size(rect.center(), vec2(side, side));
            painter.image(texture.id(), fitted, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            return;
        }
    }
    let (glyph, color) = kind_style(item.kind);
    if item.kind == AssetKind::Folder {
        let body = Rect::from_min_max(rect.min + vec2(0.0, rect.height() * 0.22), rect.max - vec2(0.0, rect.height() * 0.1));
        let tab = Rect::from_min_size(rect.min + vec2(0.0, rect.height() * 0.08), vec2(rect.width() * 0.4, rect.height() * 0.2));
        painter.rect_filled(tab, 3.0, color.gamma_multiply(0.8));
        painter.rect_filled(body, 4.0, color);
        return;
    }
    // A page with a corner marked by the kind. A kind the engine cannot use yet is dimmer.
    let usable = AssetKind::usable_by_engine(&item.name) || matches!(item.kind, AssetKind::Texture | AssetKind::Data);
    let fill = if usable { color.gamma_multiply(0.35) } else { color.gamma_multiply(0.18) };
    painter.rect_filled(rect, 5.0, fill);
    painter.rect_stroke(rect, 5.0, egui::Stroke::new(1.0_f32, color.gamma_multiply(if usable { 1.0 } else { 0.5 })), egui::StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::monospace((rect.width() * 0.3).clamp(12.0, 30.0)), color);
    // The extension, small, under the glyph.
    if let Some(ext) = item.name.rsplit_once('.').map(|(_, e)| e.to_uppercase()).filter(|e| e.len() <= 5) {
        painter.text(rect.center_bottom() - vec2(0.0, 4.0), Align2::CENTER_BOTTOM, ext, FontId::proportional(10.0), secondary_color(ui));
    }
}

/// The picture of an image file (read the first time it is needed, a few per frame, and again when the file changes).
fn thumbnail(ctx: &egui::Context, ed: &mut Editor, path: &str, size: u64, modified: Option<SystemTime>, edge: u32, budget: &mut u32) -> Option<egui::TextureHandle> {
    let key = if edge == THUMB_EDGE { path.to_owned() } else { format!("\u{1}preview:{path}") };
    if let Some(t) = ed.assets_ui.thumbs.get(&key) {
        if t.size == size && t.modified == modified {
            return t.texture.clone();
        }
    }
    if *budget == 0 {
        ctx.request_repaint();
        return None;
    }
    *budget -= 1;
    let file = ed.project.as_ref()?.root.join(path);
    let texture = (size <= MAX_THUMB_BYTES).then(|| load_image(&file, edge)).flatten().map(|image| ctx.load_texture(format!("asset:{key}"), image, egui::TextureOptions::LINEAR));
    ed.assets_ui.thumbs.insert(key, Thumb { size, modified, texture: texture.clone() });
    texture
}

/// A material file as a small shaded sphere (made the first time it is needed, and again when the file changes).
fn material_thumb(ctx: &egui::Context, ed: &mut Editor, path: &str, size: u64, modified: Option<SystemTime>, budget: &mut u32) -> Option<egui::TextureHandle> {
    let key = format!("\u{1}material:{path}");
    if let Some(t) = ed.assets_ui.thumbs.get(&key) {
        if t.size == size && t.modified == modified {
            return t.texture.clone();
        }
    }
    if *budget == 0 {
        ctx.request_repaint();
        return None;
    }
    *budget -= 1;
    let def = ed.project.as_ref().and_then(|p| p.abs(path).ok()).and_then(|f| std::fs::read(f).ok()).and_then(|b| forge_engine::assets::material::MaterialDef::parse(&b).ok()).map(|(d, _)| d);
    let texture = def.map(|d| ctx.load_texture(format!("asset:{key}"), super::material_preview::render(&d, None, 72), egui::TextureOptions::LINEAR));
    ed.assets_ui.thumbs.insert(key, Thumb { size, modified, texture: texture.clone() });
    texture
}

/// The shaded sphere of a material file, for the Material Editor's list.
pub(super) fn material_swatch(ctx: &egui::Context, ed: &mut Editor, path: &str) -> Option<egui::TextureHandle> {
    let (size, modified) = ed.asset_db().and_then(|db| db.entry(path)).map(|e| (e.size, e.modified))?;
    let mut budget = 2;
    material_thumb(ctx, ed, path, size, modified, &mut budget)
}

/// The small picture of a texture file for a property field or the picker (read the first time, a couple per frame).
pub(super) fn field_thumb(ctx: &egui::Context, ed: &mut Editor, path: &str) -> Option<egui::TextureHandle> {
    let (size, modified) = ed.asset_db().and_then(|db| db.entry(path)).map(|e| (e.size, e.modified))?;
    if !AssetKind::has_thumbnail(file_name(path)) {
        return None;
    }
    let mut budget = 2;
    thumbnail(ctx, ed, path, size, modified, THUMB_EDGE, &mut budget)
}

/// An image file as a picture no bigger than `edge` on a side (the Project Settings page uses it for the icon).
pub(super) fn load_image(file: &Path, edge: u32) -> Option<egui::ColorImage> {
    let image = image::open(file).ok()?;
    let image = if image.width() > edge || image.height() > edge { image.thumbnail(edge, edge) } else { image };
    let rgba = image.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

/// The large picture window opened by double-clicking a texture.
pub(super) fn preview_window(ctx: &egui::Context, ed: &mut Editor) {
    let Some(path) = ed.assets_ui.preview.clone() else { return };
    let Some(entry) = ed.asset_db().and_then(|db| db.entry(&path)).map(|e| (e.size, e.modified)) else {
        ed.assets_ui.preview = None;
        return;
    };
    let mut open = true;
    let mut budget = 1;
    egui::Window::new(file_name(&path).to_owned()).id(egui::Id::new("asset_preview")).open(&mut open).collapsible(false).resizable(true).default_size([420.0, 420.0]).show(ctx, |ui| {
        match thumbnail(ctx, ed, &path, entry.0, entry.1, PREVIEW_EDGE, &mut budget) {
            Some(texture) => {
                let avail = ui.available_size();
                let tex = texture.size_vec2();
                let scale = (avail.x / tex.x).min(avail.y / tex.y).min(4.0);
                ui.centered_and_justified(|ui| ui.image((texture.id(), tex * scale)));
            }
            None => {
                ui.secondary("This image cannot be shown (it may be too large or in a format the editor cannot read).");
            }
        }
    });
    if !open {
        ed.assets_ui.preview = None;
    }
}

/// Asks before deleting files (`Dialog::DeleteAssets`): lists them and what would be left pointing at them.
pub(super) fn delete_dialog(ctx: &egui::Context, ed: &mut Editor, paths: &[String], used_by: &[String]) -> bool {
    let mut keep = true;
    egui::Window::new("Delete").order(egui::Order::Foreground).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label(if paths.len() == 1 { format!("Delete {}?", paths[0]) } else { format!("Delete these {} items?", paths.len()) });
        if paths.len() > 1 {
            for p in paths.iter().take(8) {
                ui.secondary(p);
            }
            if paths.len() > 8 {
                ui.secondary(format!("... and {} more", paths.len() - 8));
            }
        }
        if !used_by.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(Color32::from_rgb(235, 175, 70), "These still use what you are deleting and will be left pointing at nothing:");
            for u in used_by.iter().take(8) {
                ui.secondary(u);
            }
            if used_by.len() > 8 {
                ui.secondary(format!("... and {} more", used_by.len() - 8));
            }
        }
        ui.add_space(4.0);
        ui.secondary("Deleted files are moved to .forge/trash in the project folder, so they can be recovered by hand.");
        ui.horizontal(|ui| {
            if ui.button("Delete").clicked() {
                ed.asset_delete(paths);
                keep = false;
            }
            if ui.button("Cancel").clicked() {
                keep = false;
            }
        });
    });
    keep
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
            let dir = std::env::temp_dir().join(format!("forge_assetui_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::remove_dir_all(&dir).ok();
            Project::create(&dir, "Asset UI Test").expect("create project");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn draw(ctx: &egui::Context, ed: &mut Editor) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 400.0))), ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| {
            egui::CentralPanel::default().show(&mut forge_engine::compat::root_ui(ctx), |ui| assets_tab(ui, ed));
            preview_window(ctx, ed);
        });
    }

    #[test]
    fn long_names_are_shortened_keeping_the_extension() {
        assert_eq!(shorten("short.obj", 12), "short.obj");
        let s = shorten("A Very Long File Name.obj", 12);
        assert!(s.chars().count() <= 12 && s.ends_with(".obj") && s.starts_with("A Ve"), "{s}");
        assert_eq!(shorten("noextensionhere", 8).chars().count(), 8);
    }

    #[test]
    fn the_browser_draws_folders_search_results_and_every_state_without_trouble() {
        let s = Scratch::new();
        let mut ed = Editor::new(Some(s.0.clone()));
        let ctx = egui::Context::default();
        draw(&ctx, &mut ed);
        draw(&ctx, &mut ed);
        assert_eq!(ed.assets_ui.dir, "Assets", "starts in the Assets folder when there is one");

        ed.browse_to("");
        draw(&ctx, &mut ed);
        ed.assets_ui.search = "type:script".into();
        draw(&ctx, &mut ed);
        ed.assets_ui.search = "zzz".into();
        draw(&ctx, &mut ed);
        ed.assets_ui.search.clear();
        ed.start_asset_rename("Client");
        draw(&ctx, &mut ed);
        ed.assets_ui.renaming = None;
        ed.assets_ui.selected = vec!["Client".into(), "Server".into(), "gone/away".into()];
        draw(&ctx, &mut ed);
        ed.assets_ui.preview = Some("Assets/missing.png".into());
        draw(&ctx, &mut ed);
        assert!(ed.assets_ui.preview.is_none(), "a preview of a file that is not there closes");
        assert!(ed.project.is_some());
    }
}
