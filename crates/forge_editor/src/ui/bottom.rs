//! The Output and Script Editor panels. (They are windows of the dock like the others, so they can sit
//! anywhere; they started life as the tabs of a bottom panel, hence the name of this file.)

use bevy_egui::egui;

use crate::state::{Editor, Level};

/// The Output panel: the log of the editor and of the running game.
pub(super) fn output(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.horizontal(|ui| {
        if ui.small_button("Clear").clicked() {
            ed.log.clear();
        }
    });
    console(ui, ed);
}
fn console(ui: &mut egui::Ui, ed: &Editor) {
    let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show_rows(ui, row_h, ed.log.len(), |ui, range| {
        for l in &ed.log[range] {
            let color = match l.level {
                Level::Info => ui.visuals().text_color(),
                Level::Warn => egui::Color32::from_rgb(230, 190, 70),
                Level::Error => egui::Color32::from_rgb(235, 100, 100),
            };
            ui.label(egui::RichText::new(&l.text).monospace().color(color));
        }
    });
}

pub(super) fn script_tab(ui: &mut egui::Ui, ed: &mut Editor) {
    let Some(s) = &mut ed.script else {
        ui.label("Open a script from the Assets tab (double-click a .luau file), or from a Script's SourceFile property.");
        return;
    };
    let mut save = false;
    let mut reload = false;
    ui.horizontal(|ui| {
        ui.strong(&s.path);
        if s.modified {
            ui.label("(modified)");
        }
        if ui.button("Save").clicked() {
            save = true;
        }
    });
    // The file was changed or removed by something else while it is open here.
    if s.missing {
        ui.colored_label(egui::Color32::from_rgb(235, 175, 70), "This file is no longer on disk. Saving writes it again.");
    }
    if s.conflict {
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::from_rgb(235, 175, 70), "This file changed on disk, and you have unsaved changes.");
            if ui.button("Load the file").on_hover_text("Discard my changes and read the file again").clicked() {
                reload = true;
            }
            if ui.button("Keep mine").on_hover_text("Saving will overwrite the file").clicked() {
                s.conflict = false;
            }
        });
    }
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        let resp = ui.add(egui::TextEdit::multiline(&mut s.text).code_editor().desired_width(f32::INFINITY).desired_rows(12).lock_focus(true));
        if resp.changed() {
            s.modified = true;
        }
    });
    if reload {
        let path = s_path(ed);
        ed.script = None;
        ed.open_script(&path);
    }
    if save {
        ed.save_all();
    }
}

fn s_path(ed: &Editor) -> String {
    ed.script.as_ref().map(|s| s.path.clone()).unwrap_or_default()
}
