//! The menu bar, the toolbar and the Insert menu. (Keyboard shortcuts are in `keymap`; what they do is in `state::actions`.)

use std::collections::BTreeMap;

use bevy_egui::egui;
use crate::theme::SecondaryText;
use forge_engine::{
    compat::{self, ExitWriter},
    core::{registry, ResolvedClass},
    project::export,
};

use crate::{
    dock::PANELS,
    keymap::Action,
    state::{Dialog, Editor, Tool},
};

/// A menu entry's text with the shortcut it currently has, e.g. `Undo  (Ctrl+Z)`.
fn labelled(ed: &Editor, text: &str, action: Action) -> String {
    match ed.settings.keymap.get(action).first() {
        Some(chord) => format!("{text}  ({})", chord.display()),
        None => text.to_owned(),
    }
}
/// Every creatable class, grouped by the category its own descriptor declares.
pub(super) fn insert_menu(ui: &mut egui::Ui, ed: &mut Editor) {
    let mut by_category: BTreeMap<&str, Vec<&'static ResolvedClass>> = BTreeMap::new();
    for class in registry().creatable() {
        by_category.entry(class.desc.category).or_default().push(class);
    }
    for (category, classes) in by_category {
        ui.menu_button(category, |ui| {
            for class in classes {
                if ui.button(class.name).on_hover_text(class.desc.description).clicked() {
                    ed.insert(class.name);
                    ui.close();
                }
            }
        });
    }
}

/// The View menu: every window of the editor with a tick for the ones that are shown, and a way to put
/// the layout back. The ticks, the dock and the window shortcuts all read the same thing (whether the panel
/// is in the layout), so they always agree. The two settings windows are listed below the panels: they are
/// windows of their own (they cannot be docked), and are ticked while open.
fn view_menu(ui: &mut egui::Ui, ed: &mut Editor) {
    ui.menu_button("View", |ui| {
        for def in PANELS.iter().filter(|d| d.panel != crate::dock::Panel::Game) {
            let panel = def.panel;
            let mut shown = ed.panel_visible(panel);
            if ui.checkbox(&mut shown, labelled(ed, def.title, Action::Window(panel))).on_hover_text(def.description).changed() {
                if shown {
                    ed.show_panel(panel);
                } else {
                    ed.hide_panel(panel);
                }
            }
        }
        ui.separator();
        let mut editor_settings = ed.editor_settings_open();
        if ui.checkbox(&mut editor_settings, labelled(ed, "Editor Settings", Action::EditorSettings)).on_hover_text("Shortcuts, camera, snapping and colours. A window of its own: it does not dock.").changed() {
            ed.toggle_editor_settings();
        }
        let mut project_settings = ed.project_settings_open();
        if ui.add_enabled(ed.project.is_some(), egui::Checkbox::new(&mut project_settings, labelled(ed, "Project Settings", Action::ProjectSettings))).on_hover_text("The project's name, start scene, window and graphics. A window of its own: it does not dock.").changed() {
            ed.toggle_project_settings();
        }
        let mut materials = ed.material_editor.open;
        if ui.add_enabled(ed.project.is_some(), egui::Checkbox::new(&mut materials, "Material Editor")).on_hover_text("Create and edit the project's materials (.mat files). A window of its own: it does not dock.").changed() {
            if materials {
                ed.open_material_editor(None);
            } else {
                ed.close_material_editor();
            }
        }
        ui.separator();
        if ui.button(labelled(ed, "Restore Default Layout", Action::RestoreLayout)).on_hover_text("Put every window back where the editor starts with it").clicked() {
            ed.restore_layout();
            ui.close();
        }
    });
}

pub(super) fn menu_bar(root: &mut egui::Ui, ed: &mut Editor, exit: &mut ExitWriter) {
    egui::Panel::top("menu").show(root, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Project...").clicked() {
                    let dir = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_default().display().to_string();
                    ed.dialog = Dialog::NewProject { name: "My Game".into(), dir };
                    ui.close();
                }
                if ui.button("Open Project...").clicked() {
                    ui.close();
                    if let Some(p) = rfd::FileDialog::new().set_title("Open a Forge project folder").pick_folder() {
                        ed.open_project(&p);
                    }
                }
                ui.separator();
                let has = ed.project.is_some();
                if ui.add_enabled(has, egui::Button::new(labelled(ed, "Save All", Action::Save))).clicked() {
                    ed.dirty = true;
                    ed.save_all();
                    ui.close();
                }
                if ui.add_enabled(has, egui::Button::new(labelled(ed, "Project Settings...", Action::ProjectSettings))).clicked() {
                    ed.open_project_settings();
                    ui.close();
                }
                if ui.button(labelled(ed, "Editor Settings...", Action::EditorSettings)).on_hover_text("Shortcuts, camera, snapping and colours").clicked() {
                    ed.open_editor_settings();
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ed.stop_play();
                    ed.save_all();
                    ed.stop();
                    compat::request_exit(exit);
                }
            });

            view_menu(ui, ed);

            let has = ed.project.is_some();
            ui.add_enabled_ui(has, |ui| {
                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled(ed.history.can_undo(), egui::Button::new(labelled(ed, "Undo", Action::Undo))).clicked() {
                        ed.undo();
                        ui.close();
                    }
                    if ui.add_enabled(ed.history.can_redo(), egui::Button::new(labelled(ed, "Redo", Action::Redo))).clicked() {
                        ed.redo();
                        ui.close();
                    }
                    ui.separator();
                    let sel = ed.selected;
                    if ui.add_enabled(sel.is_some(), egui::Button::new(labelled(ed, "Duplicate", Action::Duplicate))).clicked() {
                        ed.duplicate(sel.unwrap());
                        ui.close();
                    }
                    if ui.add_enabled(sel.is_some(), egui::Button::new(labelled(ed, "Delete", Action::Delete))).clicked() {
                        ed.delete(sel.unwrap());
                        ui.close();
                    }
                    if ui.add_enabled(sel.is_some(), egui::Button::new(labelled(ed, "Focus", Action::SnapTo))).clicked() {
                        ed.focus_request = true;
                        ui.close();
                    }
                });
                ui.menu_button("Insert", |ui| insert_menu(ui, ed));
                ui.menu_button("Scene", |ui| {
                    if ui.button("New Scene...").clicked() {
                        ed.dialog = Dialog::NewScene { name: "level".into() };
                        ui.close();
                    }
                    ui.separator();
                    for s in ed.scenes.clone() {
                        let current = ed.scene_path.as_deref() == Some(s.as_str());
                        if ui.selectable_label(current, &s).clicked() {
                            ed.load_scene(&s);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Run", |ui| {
                    let playing = ed.playing();
                    if ui.add_enabled(!playing, egui::Button::new(labelled(ed, "Play in Viewport", Action::Play))).clicked() {
                        ed.start_play();
                        ui.close();
                    }
                    if ui.add_enabled(playing, egui::Button::new(labelled(ed, "Stop", Action::Stop))).clicked() {
                        ed.stop_play();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Play in Window").clicked() {
                        ed.play_in_window();
                        ui.close();
                    }
                    if ui.add_enabled(ed.child.is_some(), egui::Button::new("Stop Window")).clicked() {
                        ed.stop();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Validate Project").clicked() {
                        ed.dirty = true;
                        ed.save_all();
                        ed.validate();
                        ui.close();
                    }
                    if ui.button("Build Standalone Game...").clicked() {
                        let out = export::default_out_dir(&ed.project.as_ref().unwrap().root).display().to_string();
                        ed.dialog = Dialog::Build { out_dir: out };
                        ui.close();
                    }
                });
            });
        });
    });
}

pub(super) fn toolbar(root: &mut egui::Ui, ed: &mut Editor) {
    egui::Panel::top("toolbar").show(root, |ui| {
        ui.horizontal(|ui| {
            let has = ed.project.is_some();
            ui.add_enabled_ui(has, |ui| {
                let playing = ed.playing();
                if playing {
                    if ui.button("Stop").on_hover_text("Stop the game and put the scene back (Shift+F5)").clicked() {
                        ed.stop_play();
                    }
                } else if ed.child.is_some() {
                    if ui.button("Stop").clicked() {
                        ed.stop();
                    }
                } else {
                    if ui.button("Play").on_hover_text("Run the game inside the viewport; you can keep editing (F5)").clicked() {
                        ed.start_play();
                    }
                    if ui.button("Play in Window").on_hover_text("Run the game in its own window").clicked() {
                        ed.play_in_window();
                    }
                }
                if ui.button("Save").clicked() {
                    ed.dirty = true;
                    ed.save_all();
                }
                if ui.add_enabled(!playing && ed.history.can_undo(), egui::Button::new("Undo")).clicked() {
                    ed.undo();
                }
                if ui.add_enabled(!playing && ed.history.can_redo(), egui::Button::new("Redo")).clicked() {
                    ed.redo();
                }
                ui.separator();
                for tool in Tool::ALL {
                    let tip = labelled(ed, tool.label(), tool.action());
                    if ui.selectable_label(ed.tool == tool, tool.label()).on_hover_text(tip).clicked() {
                        ed.tool = tool;
                    }
                }
                let space = format!("Space: {}", ed.settings.space.label());
                if ui.button(space).on_hover_text(labelled(ed, "Switch between world and local space", Action::TransformSpace)).clicked() {
                    ed.run_action(Action::TransformSpace);
                }
                if ui.selectable_label(ed.settings.render.wireframe, "Wireframe").on_hover_text(labelled(ed, "Draw the edges of every mesh over the scene", Action::Wireframe)).clicked() {
                    ed.run_action(Action::Wireframe);
                }
                if ed.settings.render.wireframe {
                    if ui.selectable_label(!ed.settings.render.wireframe_only, "Solid").on_hover_text(labelled(ed, "Draw the shaded parts under the wireframe (off: only the edges)", Action::WireframeOnly)).clicked() {
                        ed.run_action(Action::WireframeOnly);
                    }
                }
                ui.separator();
                if ui.button("Build").clicked() {
                    let out = export::default_out_dir(&ed.project.as_ref().unwrap().root).display().to_string();
                    ed.dialog = Dialog::Build { out_dir: out };
                }
            });
            ui.separator();
            match &ed.project {
                Some(p) => {
                    ui.label(format!("{}  -  {}{}", p.name(), ed.scene_path.as_deref().unwrap_or("(no scene)"), if ed.dirty { " *" } else { "" }));
                }
                None => {
                    ui.label("No project open");
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match ed.play.as_ref() {
                    Some(p) if p.game_camera && p.focused => ui.secondary("PLAYING (Game View)  |  Esc or click a panel: back to the editor  |  edits apply live and are discarded on Stop"),
                    Some(p) if p.game_camera => ui.secondary("PLAYING (Game View)  |  click the viewport to play  |  edits apply live and are discarded on Stop"),
                    Some(_) => ui.secondary("PLAYING (Editor View)  |  hold right mouse + the move keys to fly  |  edits are discarded on Stop"),
                    None => ui.secondary("Hold right mouse + the move keys to fly  |  click to select  |  File > Editor Settings > Shortcuts to change keys"),
                };
            });
        });
    });
}
