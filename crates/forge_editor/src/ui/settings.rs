//! The editor settings window (View > Editor Settings, File > Editor Settings, or the shortcut for it).
//!
//! A list of tabs on the left, grouped under dropdowns, and the chosen tab on the right. It is a window of its
//! own - it cannot be docked - that opens where it was last time at the size it was left (see `window_memory`);
//! the tabs scroll inside it instead of changing it.
//!
//! The Colors tab edits the editor's palette (see `theme`); the whole editor follows it live.
//!
//! The Shortcuts tab is generated from the table of shortcuts in `keymap`: one row per shortcut with
//! three columns (name, keys, description), a search box, and per-key editing: click a key to replace
//! it, right-click it for *Add Key* / *Remove Key*, or press `+` to add one.

use bevy_egui::egui::{self, Color32, RichText};
use crate::theme::SecondaryText;

use crate::{
    keymap::{self, Action, AddError, Captured, Chord, DEFS},
    settings::{SettingsOrder, Space, ZoomMode, MAX_FLY_SPEED, MAX_SECOND_SPEED, MIN_FLY_SPEED, MIN_SECOND_SPEED},
    state::{settings_tabs, Capturing, Editor, SettingsTab},
    theme::{parse_hex, to_hex, Palette, Rgb, Slot},
};

const ERROR: Color32 = Color32::from_rgb(235, 100, 100);
const WAITING: Color32 = Color32::from_rgb(255, 210, 90);

/// Width of the tab list on the left.
const TAB_LIST_WIDTH: f32 = 150.0;

const WINDOW: super::window_memory::Spec = super::window_memory::Spec { key: "editor_settings", title: "Editor Settings", default_size: egui::vec2(860.0, 560.0), min_size: egui::vec2(480.0, 320.0) };

/// The Editor Settings window, while it is open.
pub(super) fn editor_settings_window(ctx: &egui::Context, ed: &mut Editor) {
    if !ed.settings_ui.open {
        return;
    }
    let mut placed = ed.settings_ui.placed;
    let still_open = super::window_memory::show(ctx, ed, &WINDOW, &mut placed, editor_settings_body);
    ed.settings_ui.placed = placed;
    if !still_open {
        ed.close_editor_settings();
    }
}

/// What the Editor Settings window shows: the tab list on the left and the chosen tab on the right.
/// (Its size is whatever the user made it.)
pub(super) fn editor_settings_body(ui: &mut egui::Ui, ed: &mut Editor) {
    let height = ui.available_height();
    ui.horizontal_top(|ui| {
        // Tabs, in groups that fold away.
        ui.allocate_ui_with_layout(egui::vec2(TAB_LIST_WIDTH, height), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(TAB_LIST_WIDTH);
            egui::ScrollArea::vertical().id_salt("settings_tabs").auto_shrink([false, false]).show(ui, |ui| {
                for (group, tabs) in settings_tabs(ed.settings.settings_order) {
                    egui::CollapsingHeader::new(RichText::new(group).strong()).id_salt(("settings_group", group)).default_open(true).show(ui, |ui| {
                        for tab in tabs {
                            if ui.selectable_label(ed.settings_ui.tab == tab, tab.label()).clicked() {
                                ed.settings_ui.tab = tab;
                                ed.settings_ui.capturing = None;
                                ed.settings_ui.message = None;
                            }
                        }
                    });
                }
            });
        });
        ui.separator();
        // The tab itself: always as big as what is left of the panel, and scrolling when the content is bigger.
        // It has its own top-to-bottom layout: this row lays things out left to right, and a scroll area
        // inherits the layout it is put in, which would line the groups and rows up sideways.
        let tab = ed.settings_ui.tab;
        let size = egui::vec2(ui.available_width(), height);
        ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::both().id_salt(("settings_tab", tab.label())).auto_shrink([false, false]).show(ui, |ui| match tab {
                SettingsTab::Shortcuts => shortcuts_tab(ui, ed),
                SettingsTab::Camera => camera_tab(ui, ed),
                SettingsTab::Snapping => snapping_tab(ui, ed),
                SettingsTab::Selection => selection_tab(ui, ed),
                SettingsTab::Rendering => rendering_tab(ui, ed),
                SettingsTab::Debug => debug_tab(ui, ed),
                SettingsTab::Colors => colors_tab(ui, ed),
            });
        });
    });
}
// ---- shortcuts -------------------------------------------------------------------------------

/// Whether a row matches what was typed in the search box (name, description, window, group, id or keys).
fn matches_search(needle: &str, action: Action, chords: &[Chord]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let def = action.def();
    let squash = |s: &str| s.to_lowercase().replace(' ', "");
    let needle = squash(needle);
    [def.name, def.description, def.category, def.context.label(), def.id].iter().any(|t| squash(t).contains(&needle))
        || chords.iter().any(|c| squash(&c.display()).contains(&needle) || squash(&c.to_storage()).contains(&needle))
}

/// Every tab's content stacks top to bottom (the groups and rows below one another). The window is a
/// horizontal row (tab list | tab), so a tab that does not set its own layout would flow sideways.
fn assert_stacks_vertically(ui: &egui::Ui) {
    assert!(!ui.layout().main_dir().is_horizontal(), "the settings tab content is laid out sideways instead of top to bottom");
}

/// Draws `content` in a column of fixed width.
fn column(ui: &mut egui::Ui, width: f32, content: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(width);
        content(ui);
    });
}

fn shortcuts_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    assert_stacks_vertically(ui);
    ui.horizontal(|ui| {
        ui.label("Search");
        ui.add(egui::TextEdit::singleline(&mut ed.settings_ui.search).hint_text("a shortcut, a key (ctrl+s) or a window (viewport)").desired_width(320.0));
        if ui.small_button("x").on_hover_text("Clear the search").clicked() {
            ed.settings_ui.search.clear();
        }
        if ui.button("Reset all").on_hover_text("Put every shortcut back to its starting keys").clicked() {
            ed.settings.keymap.reset_all();
            ed.settings.touch();
            ed.settings_ui.capturing = None;
            ed.settings_ui.message = None;
        }
    });
    // How the shortcuts (and the list of settings pages on the left) are ordered. Only the order changes.
    ui.horizontal(|ui| {
        ui.label("Order");
        for order in SettingsOrder::ALL {
            if ui.selectable_label(ed.settings.settings_order == order, order.label()).on_hover_text(order.description()).clicked() && ed.settings.settings_order != order {
                ed.settings.settings_order = order;
                ed.settings.touch();
            }
        }
        ui.secondary(ed.settings.settings_order.description());
    });
    ui.secondary("Shortcuts only work while the mouse is over the window listed under their name. Click a key to replace it, right-click it to add or remove keys (Esc cancels).");
    if let Some(msg) = ed.settings_ui.message.clone() {
        ui.colored_label(ERROR, msg);
    }
    watch_for_binding(ui, ed);
    ui.separator();

    // The three columns, in this order: name, keys, description.
    let total = (ui.available_width() - 24.0).max(480.0);
    let (w_name, w_keys) = ((total * 0.24).max(150.0), (total * 0.36).max(220.0));
    let w_desc = (total - w_name - w_keys - 16.0).max(160.0);
    ui.horizontal_top(|ui| {
        column(ui, w_name, |ui| {
            ui.strong("Shortcut");
        });
        column(ui, w_keys, |ui| {
            ui.strong("Keys");
        });
        column(ui, w_desc, |ui| {
            ui.strong("Description");
        });
    });

    let needle = ed.settings_ui.search.trim().to_owned();
    let groups = shortcut_groups(ed.settings.settings_order, |action| matches_search(&needle, action, ed.settings.keymap.get(action)));
    let mut shown = 0;
    for (category, rows) in groups {
        shown += rows.len();
        // (The id is salted: "Viewport" is also the name of a group in the tab list, and two headers must not share one.)
        let header = egui::CollapsingHeader::new(RichText::new(category).strong()).id_salt(("shortcut_category", category)).default_open(true);
        // While searching, every group with a match is open.
        let header = if needle.is_empty() { header } else { header.open(Some(true)) };
        header.show(ui, |ui| {
            for action in rows {
                ui.horizontal_top(|ui| {
                    let def = action.def();
                    column(ui, w_name, |ui| {
                        ui.label(def.name);
                        ui.secondary(def.context.label());
                    });
                    column(ui, w_keys, |ui| keys_cell(ui, ed, action));
                    column(ui, w_desc, |ui| {
                        ui.add(egui::Label::new(RichText::new(def.description).color(crate::theme::secondary_color(ui))).wrap());
                    });
                });
                ui.add_space(4.0);
            }
        });
    }
    if shown == 0 {
        ui.secondary("No shortcut matches that.");
    }
}

/// The shortcuts that pass `keep`, in their groups, in the chosen order.
///
/// Common: the groups and the shortcuts in them as `keymap::DEFS` lists them - written by how people use the editor
/// (the everyday General shortcuts first, then the viewport and its tools, then the Explorer, the windows and the debug
/// overlays; related ones side by side). Alphabetical: the groups by name and the shortcuts of each by the name shown,
/// ignoring case (ties by their stable id), so the order is the same every time. Either way every shortcut is in
/// exactly one group; only where it is listed changes, never what it is or what keys it has.
pub(super) fn shortcut_groups(order: SettingsOrder, keep: impl Fn(Action) -> bool) -> Vec<(&'static str, Vec<Action>)> {
    let mut groups: Vec<(&'static str, Vec<Action>)> = Vec::new();
    for d in DEFS.iter().filter(|d| keep(d.action)) {
        match groups.iter_mut().find(|(category, _)| *category == d.category) {
            Some((_, rows)) => rows.push(d.action),
            None => groups.push((d.category, vec![d.action])),
        }
    }
    if order == SettingsOrder::Alphabetical {
        for (_, rows) in &mut groups {
            rows.sort_by_key(|a| (a.def().name.to_lowercase(), a.def().id));
        }
        groups.sort_by_key(|(category, _)| category.to_lowercase());
    }
    groups
}

/// The keys of one shortcut as small buttons, plus `+` to add one and `Reset`.
fn keys_cell(ui: &mut egui::Ui, ed: &mut Editor, action: Action) {
    let chords: Vec<Chord> = ed.settings.keymap.get(action).to_vec();
    let waiting = ed.settings_ui.capturing.filter(|c| c.action == action);

    ui.horizontal_wrapped(|ui| {
        for chord in &chords {
            let replacing = waiting.is_some_and(|w| w.replacing == Some(*chord));
            let text = if replacing { "Press keys...".to_owned() } else { chord.display() };
            let mut label = RichText::new(text).monospace();
            if replacing {
                label = label.color(WAITING);
            } else if ed.settings.keymap.clashes(action, *chord) {
                label = label.color(ERROR);
            }
            let resp = ui.add(egui::Button::new(label)).on_hover_text("Click to replace this key. Right-click for more.");
            if resp.clicked() {
                ed.settings_ui.begin_capture(action, Some(*chord));
            }
            // Right-click: the menu belongs to this key, so *Remove Key* only ever removes this one.
            resp.context_menu(|ui| {
                if ui.button("Add Key").clicked() {
                    ed.settings_ui.begin_capture(action, None);
                    ui.close();
                }
                if ui.button("Remove Key").clicked() {
                    ed.settings.keymap.remove(action, *chord);
                    ed.settings.touch();
                    ed.settings_ui.message = None;
                    if waiting.is_some_and(|w| w.replacing == Some(*chord)) {
                        ed.settings_ui.capturing = None;
                    }
                    ui.close();
                }
            });
        }
        if chords.is_empty() && waiting.is_none() {
            ui.secondary("Unbound");
        }
        if waiting.is_some_and(|w| w.replacing.is_none()) {
            let kind = action.def().kind;
            let text = match kind {
                keymap::Kind::Wheel => "Turn the wheel...",
                keymap::Kind::Hold => "Press a key or modifier...",
                keymap::Kind::Press => "Press keys...",
            };
            ui.label(RichText::new(text).monospace().color(WAITING));
        }
        if ui.small_button("+").on_hover_text("Add another key to this shortcut").clicked() {
            ed.settings_ui.begin_capture(action, None);
        }
        if !ed.settings.keymap.is_default(action) && ui.small_button("Reset").on_hover_text(format!("Starting keys: {}", starting_keys(action))).clicked() {
            ed.settings.keymap.reset(action);
            ed.settings.touch();
            if waiting.is_some() {
                ed.settings_ui.capturing = None;
            }
        }
    });
}

fn starting_keys(action: Action) -> String {
    let keys: Vec<String> = keymap::Keymap::default_of(action).iter().map(|c| c.display()).collect();
    if keys.is_empty() {
        "none".to_owned()
    } else {
        keys.join(", ")
    }
}

/// While a shortcut is waiting for its keys: reads them and applies the binding.
fn watch_for_binding(ui: &mut egui::Ui, ed: &mut Editor) {
    let Some(Capturing { action, replacing }) = ed.settings_ui.capturing else { return };
    let mut mods = ed.settings_ui.capture_mods;
    let seen = ui.ctx().input(|i| keymap::capture(i, action.def().kind, &mut mods));
    ed.settings_ui.capture_mods = mods;
    match seen {
        Captured::Waiting => {}
        Captured::Cancelled => ed.settings_ui.capturing = None,
        Captured::Chord(chord) => {
            let result = match replacing {
                Some(old) => ed.settings.keymap.replace(action, old, chord),
                None => ed.settings.keymap.add(action, chord),
            };
            match result {
                Ok(()) => {
                    ed.settings.touch();
                    ed.settings_ui.capturing = None;
                    ed.settings_ui.message = None;
                }
                Err(AddError::AlreadyHere) => {
                    ed.settings_ui.message = Some(format!("'{}' already has {}.", action.def().name, chord.display()));
                    ed.settings_ui.capture_mods = egui::Modifiers::default();
                }
                Err(AddError::UsedBy(other)) => {
                    let d = other.def();
                    ed.settings_ui.message = Some(format!("{} is already used by '{}' ({}). Remove it there first, or pick other keys.", chord.display(), d.name, d.context.label()));
                    ed.settings_ui.capture_mods = egui::Modifiers::default();
                }
            }
        }
    }
}

// ---- camera and snapping -----------------------------------------------------------------------

/// The keys of a shortcut as shown in a hint ("unbound" when it has none).
fn key_hint(ed: &Editor, action: Action) -> String {
    let keys = ed.settings.keymap.get(action);
    if keys.is_empty() {
        "unbound".to_owned()
    } else {
        keys.iter().map(|c| c.display()).collect::<Vec<_>>().join(" or ")
    }
}

fn camera_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    assert_stacks_vertically(ui);
    ui.heading("Camera");
    let (fly_wheel, second_wheel) = (key_hint(ed, Action::PrimarySpeedWheel), key_hint(ed, Action::FastSpeedWheel));
    egui::Grid::new("camera_settings").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Fly Camera Speed");
        let changed = ui
            .add(egui::DragValue::new(&mut ed.settings.camera_speed).range(MIN_FLY_SPEED..=MAX_FLY_SPEED).speed(0.2).suffix(" studs/s"))
            .on_hover_text(format!("The main speed of the fly camera. {fly_wheel} in the viewport (Adjust Fly Camera Speed) changes it too."))
            .changed();
        ui.end_row();

        ui.label("Secondary Camera Speed");
        let changed2 = ui
            .add(egui::DragValue::new(&mut ed.settings.fast_multiplier).range(MIN_SECOND_SPEED..=MAX_SECOND_SPEED).speed(0.01).prefix("x").max_decimals(2))
            .on_hover_text(format!("What the secondary speed key multiplies the fly camera speed by. Below x1 it is a slow, precise mode. {second_wheel} in the viewport (Adjust Secondary Camera Speed) changes it too."))
            .changed();
        ui.end_row();
        if changed || changed2 {
            ed.settings.touch();
        }
    });
    ui.add_space(8.0);
    ui.secondary(format!("Fly camera speed: {:.1} studs/s    Secondary camera speed held: {:.2} studs/s", ed.settings.camera_speed, ed.settings.camera_speed * ed.settings.fast_multiplier));

    ui.add_space(12.0);
    ui.label(RichText::new("Zoom behaviour").strong());
    ui.secondary("What the mouse wheel zooms toward. Either way the selection never decides it; the Focus shortcut is what frames the selection.");
    for mode in ZoomMode::ALL {
        if ui.radio_value(&mut ed.settings.zoom_mode, mode, mode.label()).on_hover_text(mode.description()).changed() {
            ed.settings.touch();
        }
        ui.indent(("zoom_mode", mode.label()), |ui| ui.secondary(mode.description()));
    }

    ui.add_space(12.0);
    if ui.button("Reset camera settings").clicked() {
        ed.settings.camera_speed = 40.0;
        ed.settings.fast_multiplier = 3.0;
        ed.settings.zoom_mode = ZoomMode::Camera;
        ed.settings.touch();
    }
}
fn snapping_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    assert_stacks_vertically(ui);
    ui.heading("Snapping");
    egui::Grid::new("snap_settings").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Move increment");
        let a = ui.add(egui::DragValue::new(&mut ed.settings.move_increment).range(0.0..=1000.0).speed(0.05).suffix(" studs")).on_hover_text("The move and scale tools snap to this (0 = free). Hold Ctrl while dragging to ignore it.").changed();
        ui.end_row();

        ui.label("Rotate increment");
        let b = ui.add(egui::DragValue::new(&mut ed.settings.rotate_increment).range(0.0..=180.0).speed(0.5).suffix(" degrees")).on_hover_text("The rotate tool snaps to this, and its ring is marked at this spacing (0 = free).").changed();
        ui.end_row();

        ui.label("Transform space");
        let mut space = ed.settings.space;
        egui::ComboBox::from_id_salt("transform_space").selected_text(space.label()).show_ui(ui, |ui| {
            ui.selectable_value(&mut space, Space::World, "World");
            ui.selectable_value(&mut space, Space::Local, "Local");
        });
        let c = space != ed.settings.space;
        ed.settings.space = space;
        ui.end_row();
        if a || b || c {
            ed.settings.touch();
        }
    });
    ui.add_space(6.0);
    ui.secondary("The shortcuts for the Move and Rotate increments open a small popup for these values; the shortcut for the transform space flips World / Local.");
}

fn rendering_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    use crate::render_settings::{VSync, MAX_LIMIT, MIN_LIMIT, MSAA_SAMPLES};
    assert_stacks_vertically(ui);
    ui.heading("Rendering");
    let mut changed = false;
    let mut render = ed.settings.render;
    egui::Grid::new("render_settings").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("V-Sync");
        egui::ComboBox::from_id_salt("vsync").selected_text(render.vsync.label()).show_ui(ui, |ui| {
            for mode in VSync::ALL {
                changed |= ui.selectable_value(&mut render.vsync, mode, mode.label()).changed();
            }
        });
        ui.end_row();

        ui.label("Frame rate limit");
        ui.horizontal(|ui| {
            let mut limited = render.fps_limit > 0;
            if ui.checkbox(&mut limited, "Limit to").changed() {
                render.fps_limit = if limited { 144 } else { 0 };
                changed = true;
            }
            ui.add_enabled_ui(limited, |ui| {
                let mut limit = render.fps_limit.max(MIN_LIMIT);
                if ui.add(egui::DragValue::new(&mut limit).range(MIN_LIMIT..=MAX_LIMIT).speed(1.0).suffix(" FPS")).on_hover_text("The editor waits between frames so it does not go faster than this").changed() {
                    render.fps_limit = limit;
                    changed = true;
                }
            });
            if !limited {
                ui.secondary("unlimited");
            }
        });
        ui.end_row();

        ui.label("Anti-aliasing (MSAA)");
        egui::ComboBox::from_id_salt("msaa").selected_text(if render.msaa == 1 { "Off".to_owned() } else { format!("{}x", render.msaa) }).show_ui(ui, |ui| {
            for samples in MSAA_SAMPLES {
                let label = if samples == 1 { "Off".to_owned() } else { format!("{samples}x") };
                changed |= ui.selectable_value(&mut render.msaa, samples, label).changed();
            }
        });
        ui.end_row();

        ui.label("Viewport grid");
        changed |= ui.checkbox(&mut render.show_grid, "Show Viewport Grid").on_hover_text("Draw the grid lines and the axes on the ground of the viewport. Only the picture changes: snapping, the tools, selecting and the camera work the same either way.").changed();
        ui.end_row();

        ui.label("Wireframe");
        changed |= ui.checkbox(&mut render.wireframe, "Draw the edges of every mesh").on_hover_text("Also the Wireframe button in the toolbar, or Z over the viewport. Needs a graphics API that can draw polygon edges (Vulkan, DirectX 12 or Metal). Every part is drawn twice, so a big scene gets slower.").changed();
        ui.end_row();

        ui.label("Wireframe only");
        ui.add_enabled_ui(render.wireframe, |ui| {
            changed |= ui.checkbox(&mut render.wireframe_only, "Hide the shaded parts").on_hover_text("Draw only the edges (Shift+Z over the viewport, or the Solid button in the toolbar).").changed();
        });
        ui.end_row();

        ui.label("Occlusion culling");
        changed |= ui.checkbox(&mut render.occlusion_culling, "Skip what is hidden behind other objects").on_hover_text("Not used in the editor's own viewport (Bevy hides visible objects for a camera that draws into part of the window): it applies to games run with Play in Window or the player. Needs a graphics API that can build draw lists on the GPU (Vulkan or Metal, not DirectX 12); on others this does nothing. The start-up log says whether it is available. Off by default: it adds a depth pass, which only pays off when the GPU is the bottleneck (a lot of hidden overdraw) and anti-aliasing is off. With many objects and the CPU as the bottleneck, or with MSAA, it makes frames slower.").changed();
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.add_enabled(!render.is_uncapped(), egui::Button::new("Uncap frame rate")).on_hover_text("V-Sync off and no limit: the editor draws as many frames as it can").clicked() {
            render.uncap();
            changed = true;
        }
        if ui.add_enabled(render != crate::render_settings::RenderSettings::default(), egui::Button::new("Reset rendering settings")).clicked() {
            render = crate::render_settings::RenderSettings::default();
            changed = true;
        }
    });
    ui.add_space(6.0);
    ui.secondary("With V-Sync on, the frame rate follows the monitor. Off (uncapped) lets it run as fast as the computer can, which can show tearing and keeps the GPU busy. Low latency (Mailbox) shows the newest finished frame at every refresh. To watch the frame rate, switch on Show FPS in the Debug menu (Ctrl+Shift+F5).");
    if changed {
        render.sanitize();
        ed.settings.render = render;
        ed.settings.touch();
    }
}

fn debug_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    assert_stacks_vertically(ui);
    ui.heading("Debug overlays");
    ui.secondary("Switch overlays on and off in the Debug menu above the viewport, or with their shortcuts (the Debug group in the Shortcuts tab). These settings shape how they look and behave.");
    ui.add_space(6.0);
    if forge_engine::diag::ui::config(ui, &mut ed.settings.debug) {
        ed.settings.touch();
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let any = !ed.settings.debug.enabled.is_empty();
        if ui.add_enabled(any, egui::Button::new("Disable all overlays")).clicked() {
            ed.settings.debug.disable_all();
            ed.settings.touch();
        }
        if ui.add_enabled(!ed.settings.debug.is_default(), egui::Button::new("Reset debug settings")).on_hover_text("Every overlay off, every setting back to its default").clicked() {
            ed.settings.debug.reset();
            ed.settings.touch();
        }
        if ui.button("Clear collected data").on_hover_text("Forget the frame history, peaks, script totals and recorded warnings").clicked() {
            ed.clear_debug_data = true;
        }
    });
    ui.add_space(6.0);
    ui.secondary(format!("{} of {} overlays are on.", ed.settings.debug.enabled.len(), forge_engine::diag::OVERLAYS.len()));
}

// ---- colors ------------------------------------------------------------------------------------

/// The `#RRGGBB` text box next to a swatch. While it is being typed in it keeps the text as typed (a
/// half-written code is not a colour yet); otherwise it shows the colour. Returns whether the colour changed.
fn hex_field(ui: &mut egui::Ui, slot: Slot, rgb: &mut Rgb) -> bool {
    let id = ui.make_persistent_id(("color_hex", slot.key()));
    let typing = ui.memory(|m| m.has_focus(id));
    let mut text = if typing { ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| to_hex(*rgb)) } else { to_hex(*rgb) };
    let response = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(78.0).char_limit(7).font(egui::TextStyle::Monospace));
    let mut changed = false;
    if response.changed() {
        if let Some(parsed) = parse_hex(&text) {
            changed = parsed != *rgb;
            *rgb = parsed;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, text));
    changed
}

fn colors_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    assert_stacks_vertically(ui);
    ui.heading("Colors");
    ui.secondary("The colours of the whole editor. A change shows at once. Click a swatch to pick a colour, or type a code like #F59E0B.");
    ui.add_space(6.0);
    let mut changed = false;
    egui::Grid::new("color_settings").num_columns(5).spacing([16.0, 8.0]).show(ui, |ui| {
        for slot in Slot::ALL {
            let mut rgb = ed.settings.theme.get(slot);
            ui.label(slot.label()).on_hover_text(slot.description());
            let mut edited = ui.color_edit_button_srgb(&mut rgb).changed();
            edited |= hex_field(ui, slot, &mut rgb);
            let reset = ui.add_enabled(rgb != slot.default_rgb(), egui::Button::new("Reset")).on_hover_text(format!("Back to {}", to_hex(slot.default_rgb())));
            if reset.clicked() {
                rgb = slot.default_rgb();
                edited = true;
            }
            ui.secondary(slot.description());
            ui.end_row();
            if edited {
                ed.settings.theme.set(slot, rgb);
                changed = true;
            }
        }
    });
    ui.add_space(8.0);
    if ui.add_enabled(!ed.settings.theme.is_default(), egui::Button::new("Reset all colors")).on_hover_text("Put every colour back to its starting value").clicked() {
        ed.settings.theme = Palette::default();
        changed = true;
    }
    if changed {
        ed.settings.touch();
    }
}

fn selection_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    assert_stacks_vertically(ui);
    ui.heading("Selection");
    egui::Grid::new("selection_settings").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label("Alt-click tolerance");
        let changed = ui
            .add(egui::DragValue::new(&mut ed.settings.click_cycle_tolerance).range(0.0..=50.0).speed(0.1).suffix(" px"))
            .on_hover_text("Alt-clicking again within this distance of the last Alt-click counts as the same spot, and selects the next object behind the last one.")
            .changed();
        ui.end_row();
        if changed {
            ed.settings.touch();
        }
    });
    ui.add_space(6.0);
    ui.secondary("Click selects an object - or the whole group (Model) when it is a part of one. Shift + click adds or removes it. Alt + click selects the part under the mouse itself, even inside a group, and keeps walking back through what is behind it on repeated clicks. Shift + Alt + click does the same while keeping the objects already selected. In the Explorer, Ctrl + click adds or removes one object and Shift + click selects every row from the last one clicked (Ctrl + Shift keeps what is already selected).");
}

#[cfg(test)]
mod tests {
    use forge_engine::compat::RunFrame as _;
    use super::*;
    use crate::keymap::Kind;

    fn raw(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() }
    }

    /// One frame of the settings panel's contents, as the dock draws them.
    fn draw(ctx: &egui::Context, input: egui::RawInput, ed: &mut Editor) {
        let _ = ctx.run_frame(input, |ctx| {
            egui::CentralPanel::default().show(&mut forge_engine::compat::root_ui(ctx), |ui| editor_settings_body(ui, ed));
        });
    }

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, ed: &mut Editor) {
        draw(ctx, raw(events), ed);
    }

    fn key(k: egui::Key, m: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m }
    }

    fn open_editor() -> Editor {
        Editor::new(None)
    }

    fn c(text: &str) -> Chord {
        Chord::parse(text).unwrap()
    }

    #[test]
    fn search_finds_shortcuts_by_name_description_window_group_and_keys() {
        let map = crate::keymap::Keymap::default();
        let hits = |needle: &str| -> Vec<Action> { DEFS.iter().map(|d| d.action).filter(|a| matches_search(needle, *a, map.get(*a))).collect() };
        assert_eq!(hits("").len(), DEFS.len(), "an empty search shows everything");
        assert!(hits("rename").contains(&Action::Rename) && !hits("rename").contains(&Action::Save));
        assert!(hits("CTRL + S").contains(&Action::Save), "key names ignore case and spaces");
        assert!(hits("ctrl+y").contains(&Action::Redo), "every key of a shortcut is searched");
        assert!(hits("ctrl+shift+g").contains(&Action::Folder) && !hits("ctrl+shift+g").contains(&Action::Group));
        let viewport = hits("viewport");
        assert!(viewport.contains(&Action::CamForward) && viewport.contains(&Action::ZoomWheel) && !viewport.contains(&Action::Save));
        assert!(hits("scene tree").contains(&Action::Ungroup), "group names match too");
        assert!(hits("trackball").contains(&Action::ToolRotate), "descriptions are searched");
        assert!(hits("zzzz").is_empty());
    }

    #[test]
    fn every_tab_draws() {
        let ctx = egui::Context::default();
        let mut ed = open_editor();
        for order in SettingsOrder::ALL {
            ed.settings.settings_order = order;
            for (_, tabs) in crate::state::SETTINGS_TABS {
                for tab in *tabs {
                    ed.settings_ui.tab = *tab;
                    frame(&ctx, vec![], &mut ed);
                    frame(&ctx, vec![], &mut ed);
                }
            }
        }
    }

    #[test]
    fn common_order_puts_the_everyday_shortcuts_first_and_alphabetical_sorts_by_the_names_shown() {
        let all = |order| shortcut_groups(order, |_| true);
        let common = all(SettingsOrder::Common);
        let names: Vec<&str> = common.iter().map(|(g, _)| *g).collect();
        assert_eq!(names, ["General", "Viewport", "Tools", "Scene Tree", "Windows", "Debug"], "by how people work: what is used all the time first");
        assert_eq!(common[0].1.first(), Some(&Action::Save), "Save leads the General group");
        let general: Vec<Action> = common[0].1.clone();
        let at = |a: Action| general.iter().position(|x| *x == a).unwrap();
        assert!(at(Action::Duplicate) < at(Action::RepeatDuplicate) && at(Action::RepeatDuplicate) < at(Action::Delete), "related shortcuts are side by side");
        assert!(at(Action::Undo) < at(Action::Cut) && at(Action::Cut) < at(Action::Paste));

        let alphabetical = all(SettingsOrder::Alphabetical);
        let groups: Vec<String> = alphabetical.iter().map(|(g, _)| g.to_lowercase()).collect();
        let mut sorted = groups.clone();
        sorted.sort();
        assert_eq!(groups, sorted, "the groups A to Z");
        for (group, rows) in &alphabetical {
            let names: Vec<String> = rows.iter().map(|a| a.def().name.to_lowercase()).collect();
            let mut sorted = names.clone();
            sorted.sort();
            assert_eq!(names, sorted, "{group}: the shortcuts A to Z by the name shown, whatever the case");
        }
        // The same shortcuts in the same groups, whichever order.
        let flat = |groups: &[(&'static str, Vec<Action>)]| {
            let mut v: Vec<(&'static str, &'static str)> = groups.iter().flat_map(|(g, rows)| rows.iter().map(move |a| (*g, a.def().id))).collect();
            v.sort();
            v
        };
        assert_eq!(flat(&common), flat(&alphabetical));
        assert_eq!(flat(&common).len(), DEFS.len(), "every shortcut is listed exactly once");
        assert_eq!(all(SettingsOrder::Alphabetical), alphabetical, "and the same every time");
        // A search keeps its matches in either order.
        let only_save = shortcut_groups(SettingsOrder::Alphabetical, |a| a == Action::Save);
        assert_eq!(only_save, [("General", vec![Action::Save])]);
    }

    #[test]
    fn switching_the_order_changes_nothing_but_the_order_and_is_remembered() {
        let ctx = egui::Context::default();
        let mut ed = open_editor();
        assert_eq!(ed.settings.settings_order, SettingsOrder::Common, "Common is the default");
        ed.settings.keymap.add(Action::Save, c("Ctrl+Alt+W")).unwrap();
        ed.settings.camera_speed = 77.0;
        let (keys, speed) = (ed.settings.keymap.overrides(), ed.settings.camera_speed);
        let tabs = |ed: &Editor| settings_tabs(ed.settings.settings_order);
        let common_tabs = tabs(&ed);
        assert_eq!(common_tabs[0].0, "Input", "the keys first");

        ed.settings.settings_order = SettingsOrder::Alphabetical;
        for tab in [SettingsTab::Shortcuts, SettingsTab::Camera, SettingsTab::Rendering] {
            ed.settings_ui.tab = tab;
            frame(&ctx, vec![], &mut ed);
        }
        let alphabetical_tabs = tabs(&ed);
        assert_eq!(alphabetical_tabs.iter().map(|(g, _)| *g).collect::<Vec<_>>(), ["Debug", "Input", "Interface", "Viewport"]);
        assert_eq!(alphabetical_tabs.iter().find(|(g, _)| *g == "Viewport").unwrap().1, [SettingsTab::Camera, SettingsTab::Rendering, SettingsTab::Selection, SettingsTab::Snapping]);
        let flat = |t: &[(&'static str, Vec<SettingsTab>)]| {
            let mut v: Vec<(&'static str, &'static str)> = t.iter().flat_map(|(g, tabs)| tabs.iter().map(move |t| (*g, t.label()))).collect();
            v.sort();
            v
        };
        assert_eq!(flat(&common_tabs), flat(&alphabetical_tabs), "the same pages");
        assert_eq!((ed.settings.keymap.overrides(), ed.settings.camera_speed), (keys, speed), "no key and no value changed");
    }

    #[test]
    fn the_order_is_saved_with_the_editor_settings() {
        let dir = std::env::temp_dir().join(format!("forge_order_test_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let path = dir.join("editor_settings.json");
        let (mut s, _) = crate::settings::EditorSettings::load_at(path.clone());
        assert_eq!(s.settings_order, SettingsOrder::Common);
        s.settings_order = SettingsOrder::Alphabetical;
        s.touch();
        assert_eq!(s.save_if_changed(), None);
        let (back, problem) = crate::settings::EditorSettings::load_at(path.clone());
        assert!(problem.is_none());
        assert_eq!(back.settings_order, SettingsOrder::Alphabetical, "the next session has it");
        std::fs::write(&path, r#"{ "settings_order": "FromTheFuture" }"#).unwrap();
        assert_eq!(crate::settings::EditorSettings::load_at(path.clone()).0.settings_order, SettingsOrder::Common, "an unknown one reads as the default");
        std::fs::write(&path, r#"{ "camera_speed": 50 }"#).unwrap();
        assert_eq!(crate::settings::EditorSettings::load_at(path).0.settings_order, SettingsOrder::Common, "a file from before it existed too");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_new_key_is_added_next_to_the_existing_ones() {
        let ctx = egui::Context::default();
        let mut ed = open_editor();
        ed.settings_ui.begin_capture(Action::Save, None);
        let mods = egui::Modifiers { ctrl: true, alt: true, command: true, ..Default::default() };
        frame(&ctx, vec![], &mut ed);
        frame(&ctx, vec![key(egui::Key::W, mods)], &mut ed);
        assert_eq!(ed.settings.keymap.get(Action::Save), [c("Ctrl+S"), c("Ctrl+Alt+W")]);
        assert!(ed.settings_ui.capturing.is_none(), "capturing ends once a key is taken");
        assert_eq!(ed.settings.keymap.overrides().len(), 1);
    }

    #[test]
    fn clicking_a_key_replaces_only_that_key() {
        let ctx = egui::Context::default();
        let mut ed = open_editor();
        ed.settings_ui.begin_capture(Action::Redo, Some(c("Ctrl+Y")));
        let mods = egui::Modifiers { ctrl: true, alt: true, command: true, ..Default::default() };
        frame(&ctx, vec![], &mut ed);
        frame(&ctx, vec![key(egui::Key::H, mods)], &mut ed);
        assert_eq!(ed.settings.keymap.get(Action::Redo), [c("Ctrl+Shift+Z"), c("Ctrl+Alt+H")]);
    }

    #[test]
    fn a_key_that_clashes_or_repeats_is_refused_with_a_reason_and_escape_cancels() {
        let ctx = egui::Context::default();
        let mut ed = open_editor();
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        // Copy is Ctrl+C; try to give Paste the same keys (both work anywhere, so they would fight).
        ed.settings_ui.begin_capture(Action::Paste, None);
        frame(&ctx, vec![], &mut ed);
        frame(&ctx, vec![key(egui::Key::C, ctrl)], &mut ed);
        assert!(ed.settings.keymap.is_default(Action::Paste), "the clashing key was not applied");
        let message = ed.settings_ui.message.clone().expect("the user is told why");
        assert!(message.contains("Copy"), "{message}");
        assert!(ed.settings_ui.capturing.is_some(), "still waiting for other keys");

        // A key the shortcut already has.
        frame(&ctx, vec![key(egui::Key::V, ctrl)], &mut ed);
        assert!(ed.settings_ui.message.clone().unwrap().contains("already has"));
        assert_eq!(ed.settings.keymap.get(Action::Paste).len(), 1);

        frame(&ctx, vec![key(egui::Key::Escape, egui::Modifiers::default())], &mut ed);
        assert!(ed.settings_ui.capturing.is_none());
        assert!(ed.settings.keymap.is_default(Action::Paste));
    }

    #[test]
    fn the_same_keys_may_be_used_in_different_windows() {
        let ctx = egui::Context::default();
        let mut ed = open_editor();
        // Rename (Explorer) takes Ctrl+T, which rotates in the Viewport: different windows, so it is allowed.
        ed.settings_ui.begin_capture(Action::Rename, None);
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        frame(&ctx, vec![], &mut ed);
        frame(&ctx, vec![key(egui::Key::T, ctrl)], &mut ed);
        assert!(ed.settings.keymap.has(Action::Rename, c("Ctrl+T")));
        assert!(ed.settings.keymap.has(Action::Rotate90X, c("Ctrl+T")));
    }

    #[test]
    fn a_held_shortcut_can_become_a_lone_modifier_and_a_wheel_shortcut_takes_the_wheel() {
        let ctx = egui::Context::default();
        let mut ed = open_editor();
        assert_eq!(Action::FastCamera.def().kind, Kind::Hold);
        ed.settings_ui.begin_capture(Action::FastCamera, None);
        let alt = egui::Modifiers { alt: true, ..Default::default() };
        let with = |mods, events: Vec<egui::Event>| {
            let mut input = raw(events);
            input.events.insert(0, egui::Event::ModifiersChanged(mods));
            input
        };
        draw(&ctx, with(alt, vec![]), &mut ed);
        draw(&ctx, with(egui::Modifiers::default(), vec![]), &mut ed);
        assert!(ed.settings.keymap.has(Action::FastCamera, c("Alt")));

        ed.settings_ui.begin_capture(Action::ZoomWheel, None);
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        let wheel = vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, 1.0), phase: egui::TouchPhase::Move, modifiers: ctrl }];
        draw(&ctx, with(ctrl, wheel), &mut ed);
        assert!(ed.settings.keymap.has(Action::ZoomWheel, c("Ctrl+Scroll")));
    }

    #[test]
    fn closing_the_window_stops_capturing() {
        let ctx = egui::Context::default();
        let mut ed = open_editor();
        ed.open_editor_settings();
        ed.settings_ui.begin_capture(Action::Undo, None);
        frame(&ctx, vec![], &mut ed);
        assert!(ed.settings_ui.capturing.is_some(), "still waiting while the window is shown");
        ed.close_editor_settings();
        assert!(ed.settings_ui.capturing.is_none());
    }

    #[test]
    fn the_camera_tab_names_the_speeds_and_lets_the_zoom_behaviour_be_chosen() {
        let ctx = egui::Context::default();
        let mut ed = open_editor();
        ed.settings_ui.tab = SettingsTab::Camera;
        frame(&ctx, vec![], &mut ed);
        let text = ctx.run_frame(raw(vec![]), |ctx| {
            egui::CentralPanel::default().show(&mut forge_engine::compat::root_ui(ctx), |ui| editor_settings_body(ui, &mut ed));
        });
        let shown: String = text.shapes.iter().filter_map(|s| if let egui::epaint::Shape::Text(t) = &s.shape { Some(t.galley.text().to_owned()) } else { None }).collect::<Vec<_>>().join("|");
        for expected in ["Fly Camera Speed", "Secondary Camera Speed", "Zoom behaviour", "Camera-Based Zoom", "Hover-Based Zoom"] {
            assert!(shown.contains(expected), "{expected} is on the Camera tab: {shown}");
        }
        assert_eq!(ed.settings.zoom_mode, ZoomMode::Camera);
    }
}
