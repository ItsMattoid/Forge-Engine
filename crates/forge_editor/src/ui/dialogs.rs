//! The welcome window and the modal dialogs (new project, build, new scene, settings).

use std::path::PathBuf;

use bevy_egui::egui;
use crate::theme::SecondaryText;
use forge_engine::project::slugify;

use crate::state::{Dialog, Editor};

pub(super) fn welcome(ctx: &egui::Context, ed: &mut Editor) {
    if ed.project.is_some() || !matches!(ed.dialog, Dialog::None) {
        return;
    }
    egui::Window::new("Welcome to Forge").order(egui::Order::Foreground).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label("Create a new game or open an existing project.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("New Project...").clicked() {
                let dir = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_default().display().to_string();
                ed.dialog = Dialog::NewProject { name: "My Game".into(), dir };
            }
            if ui.button("Open Project...").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    ed.open_project(&p);
                }
            }
        });
    });
}

pub(super) fn dialogs(ctx: &egui::Context, ed: &mut Editor) {
    let mut dialog = std::mem::replace(&mut ed.dialog, Dialog::None);
    let mut keep = true;
    let window = |title: &str| egui::Window::new(title.to_owned()).order(egui::Order::Foreground).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]);

    match &mut dialog {
        Dialog::None => {}
        Dialog::NewProject { name, dir } => {
            window("New Project").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name    ");
                    ui.text_edit_singleline(name);
                });
                ui.horizontal(|ui| {
                    ui.label("Location");
                    ui.text_edit_singleline(dir);
                    if ui.button("Browse...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            *dir = p.display().to_string();
                        }
                    }
                });
                let target = PathBuf::from(dir.as_str()).join(slugify(name));
                ui.secondary(format!("Creates {}", target.display()));
                ui.horizontal(|ui| {
                    let ok = !name.trim().is_empty() && !dir.trim().is_empty();
                    if ui.add_enabled(ok, egui::Button::new("Create")).clicked() {
                        ed.create_project(&target, name.trim());
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        }
        Dialog::Build { out_dir } => {
            window("Build Standalone Game").show(ctx, |ui| {
                ui.label("Packages the game and the player executable into a folder you can ship.");
                ui.horizontal(|ui| {
                    ui.label("Output");
                    ui.text_edit_singleline(out_dir);
                    if ui.button("Browse...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            *out_dir = p.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(!out_dir.trim().is_empty(), egui::Button::new("Build")).clicked() {
                        ed.build(PathBuf::from(out_dir.as_str()));
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        }
        Dialog::NewScene { name } => {
            window("New Scene").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(name);
                });
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() {
                        ed.new_scene(name);
                        keep = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        }
        Dialog::DeleteAssets { paths, used_by } => {
            keep = super::assets::delete_dialog(ctx, ed, paths, used_by);
        }
    }

    if keep {
        ed.dialog = dialog;
    }
}
